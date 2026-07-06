#![no_std]

use embedded_graphics::{
    mono_font::{MonoFont, MonoTextStyleBuilder},
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle, RoundedRectangle, Triangle},
    text::{Baseline, Text},
};

use knurl_core::{Area, BorderStyle, RenderTarget, Style};

pub use knurl_core as core;

/// A gentle corner radius (in pixels) for a `RoundedRectangle` of `size`, used by
/// rounded borders and the rounded fill bars. Scales with the smaller side and is
/// clamped so it never exceeds half the box.
fn corner_radius(size: Size) -> u32 {
    let m = size.width.min(size.height);
    (m / 5).min(8).min(m / 2)
}

/// Top-left pixel and side length for a square check/radio indicator that fills
/// the area height (with a 1px breathing gap top/bottom, like the bars) and is
/// left-aligned in `rect`. Clamped to the rect width so it never overflows.
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
            (0x01, 0, 0), (0x02, 0, 1), (0x04, 0, 2), (0x40, 0, 3),
            (0x08, 1, 0), (0x10, 1, 1), (0x20, 1, 2), (0x80, 1, 3),
        ];
        for (bit, cx, ry) in MAP {
            if bits & bit != 0 {
                let p = tl + Point::new((cx * col_w) as i32, (ry * row_h) as i32);
                let _ = Rectangle::new(p, Size::new(dot, dot)).into_styled(fill).draw(display);
            }
        }
        true
    } else if let Some(quarters) = block_fraction(frame) {
        // Pulse/Meter: a centred square scaled by the shade fraction.
        let side = (w.min(h) * quarters / 4).max(1);
        let p = tl + Point::new(((w - side) / 2) as i32, ((h - side) / 2) as i32);
        let _ = Rectangle::new(p, Size::new(side, side)).into_styled(fill).draw(display);
        true
    } else {
        false
    }
}

/// A small filled triangle for a tree expander: pointing **down** when
/// `expanded`, **right** when collapsed, fitting an `s`-pixel square at `top`.
fn expander_triangle(top: Point, s: u32, expanded: bool) -> Triangle {
    let s = s as i32;
    let (x, y) = (top.x, top.y);
    if expanded {
        Triangle::new(Point::new(x, y), Point::new(x + s, y), Point::new(x + s / 2, y + s))
    } else {
        Triangle::new(Point::new(x, y), Point::new(x, y + s), Point::new(x + s, y + s / 2))
    }
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
        Self { inverted: Self::INVERTED | Self::FOCUS, blink: 0, blink_on: true }
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
}

impl<'a, D: DrawTarget<Color = BinaryColor>> GraphicsTarget<'a, D> {
    pub fn new(display: &'a mut D, font: MonoFont<'static>) -> Self {
        Self { display, font, theme: Theme::new() }
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
        Rectangle::new(self.px_point(area.x, area.y), Size::new(area.w as u32, area.h as u32))
    }

    /// Access to the underlying `DrawTarget` for pixel-level widget drawing.
    pub fn display_mut(&mut self) -> &mut D {
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
        // Compute the pixel origin before borrowing `self.font`/`self.display`
        // simultaneously - the bounding_box() borrow ends here (NLL).
        let pos = self.px_point(x, y);

        let inverted = self.theme.resolve(style);
        let (text_color, bg_color) = if inverted {
            (BinaryColor::Off, BinaryColor::On)
        } else {
            (BinaryColor::On, BinaryColor::Off)
        };

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
    /// | `Thick`        | 2-px stroked rectangle                      |
    /// | `Double`       | two concentric 1-px rectangles, 2-px apart  |
    fn draw_box(&mut self, area: Area, border: BorderStyle) {
        if matches!(border, BorderStyle::None) {
            return;
        }

        let rect = self.px_rect(area);
        let top_left = rect.top_left;
        let size = rect.size;

        match border {
            BorderStyle::None => unreachable!(),

            BorderStyle::Single => {
                let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
                let _ = Rectangle::new(top_left, size).into_styled(style).draw(self.display);
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
                let style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);
                let _ = Rectangle::new(top_left, size).into_styled(style).draw(self.display);
            }

            BorderStyle::Double => {
                let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
                let _ = Rectangle::new(top_left, size).into_styled(stroke).draw(self.display);

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
        let rect = self.px_rect(area);
        let style = PrimitiveStyle::with_fill(BinaryColor::Off);
        let _ = rect.into_styled(style).draw(self.display);
    }

    /// Fill the pixel region solid. Monochrome has no colour, so `style` is
    /// ignored - the fill is always `On`.
    fn fill_rect(&mut self, area: Area, _style: Style) {
        let rect = self.px_rect(area);
        let style = PrimitiveStyle::with_fill(BinaryColor::On);
        let _ = rect.into_styled(style).draw(self.display);
    }

    /// A smooth pixel bar: a rounded outline track with a solid rounded fill of
    /// width `fill_permille/1000`. Monochrome has no colour, so `style` is
    /// ignored - fill and track are both `On` (outline vs solid distinguishes
    /// them).
    fn draw_bar(&mut self, area: Area, fill_permille: u16, _style: Style) {
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

    /// A rounded square checkbox; filled inner square when `on`. Monochrome:
    /// `style` ignored (always `On`).
    fn draw_check(&mut self, area: Area, on: bool, _style: Style) {
        let (top, s) = indicator_square(self.px_rect(area));
        if s == 0 {
            return;
        }
        let bx = Rectangle::new(top, Size::new(s, s));
        let r = corner_radius(bx.size);
        let _ = RoundedRectangle::with_equal_corners(bx, Size::new(r, r))
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
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
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(self.display);
            }
        }
    }

    /// A circle radio; filled centre dot when `on`. Monochrome: `style` ignored.
    fn draw_radio(&mut self, area: Area, on: bool, _style: Style) {
        let (top, d) = indicator_square(self.px_rect(area));
        if d == 0 {
            return;
        }
        let _ = Circle::new(top, d)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(self.display);
        if on {
            let inset = (d / 4).max(1);
            if d > inset * 2 {
                let dot = Circle::new(top + Point::new(inset as i32, inset as i32), d - inset * 2);
                let _ = dot
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(self.display);
            }
        }
    }

    /// A filled triangle expander (down = expanded, right = collapsed).
    /// Monochrome: `style` ignored (always `On`).
    fn draw_expander(&mut self, area: Area, expanded: bool, _style: Style) {
        let (top, s) = indicator_square(self.px_rect(area));
        if s == 0 {
            return;
        }
        let _ = expander_triangle(top, s, expanded)
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(self.display);
    }

    /// Pixel spinner frame (Braille dot matrix / pulsing block); Line-style glyphs
    /// fall back to text. Monochrome: `style` ignored (always `On`).
    fn draw_spinner(&mut self, area: Area, frame: char, style: Style) {
        let tl = self.px_point(area.x, area.y);
        if !spinner_pixels(self.display, tl, area, frame, BinaryColor::On) {
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
            danger: (danger, bg),         // danger text, not a block
            focus: (lilac, dark_grey),    // selection focus
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
        let text = Rgb565::new(27, 55, 29);       // #D8DEE9
        let bg = Rgb565::new(5, 13, 8);           // #2E3440
        let muted = Rgb565::new(9, 21, 13);       // #4C566A
        let accent = Rgb565::new(17, 48, 26);     // #88C0D0
        let snow = Rgb565::new(28, 58, 30);       // #E5E9F0
        let dark_grey = Rgb565::new(8, 19, 11);   // #434C5E
        let danger = Rgb565::new(23, 24, 13);     // #BF616A
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
        let text = Rgb565::new(31, 62, 30);       // #F8F8F2
        let bg = Rgb565::new(5, 10, 6);           // #282A36
        let muted = Rgb565::new(12, 28, 20);      // #6272A4
        let accent = Rgb565::new(23, 36, 31);     // #BD93F9
        let selection = Rgb565::new(8, 17, 11);   // #44475A
        let danger = Rgb565::new(31, 21, 10);     // #FF5555
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
        let text = Rgb565::new(29, 54, 22);       // #EBDBB2
        let bg = Rgb565::new(5, 10, 5);           // #282828
        let muted = Rgb565::new(18, 32, 14);      // #928374
        let accent = Rgb565::new(31, 47, 5);      // #FABD2F
        let light_sand = Rgb565::new(31, 60, 24); // #FBF1C7
        let bark = Rgb565::new(10, 18, 8);        // #504945
        let danger = Rgb565::new(31, 18, 6);       // #FB4934
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
        let text = Rgb565::new(31, 63, 31);       // #FFFFFF
        let bg = Rgb565::new(0, 0, 0);            // #000000
        let muted = Rgb565::new(16, 32, 16);      // #808080
        let accent = Rgb565::new(0, 63, 6);       // #00FF33 (r>>3=0, g>>2=63, b>>3=6)
        let danger = Rgb565::new(31, 0, 6);       // #FF0033
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
        let cyan = Rgb565::new(0, 60, 31);        // #00F0FF
        let bg = Rgb565::new(1, 3, 2);            // #0A0E17
        let muted = Rgb565::new(11, 26, 15);      // #5D6978
        let crimson = Rgb565::new(31, 0, 10);     // #FF0055
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
        let text = Rgb565::new(31, 63, 31);       // #FDFDFD
        let bg = Rgb565::new(4, 8, 6);            // #262335
        let muted = Rgb565::new(16, 34, 23);      // #848BB8
        let accent = Rgb565::new(31, 31, 27);     // #FF7EDB
        let yellow = Rgb565::new(31, 62, 13);     // #FEFA6B
        let dark_purple = Rgb565::new(6, 10, 12); // #372963
        let danger = Rgb565::new(31, 17, 10);     // #FE4450
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
        let text = Rgb565::new(21, 44, 26);       // #A9B1D6
        let bg = Rgb565::new(3, 6, 4);            // #1A1B26
        let muted = Rgb565::new(10, 23, 17);      // #565F89
        let accent = Rgb565::new(15, 40, 30);     // #7AA2F7
        let cyan = Rgb565::new(14, 54, 25);       // #73DACA
        let focus_bg = Rgb565::new(6, 17, 15);    // #33467C
        let danger = Rgb565::new(30, 29, 17);     // #F7768E
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
        let text = Rgb565::new(26, 49, 21);       // #D3C6AA
        let bg = Rgb565::new(3, 8, 4);            // #1E2326
        let muted = Rgb565::new(15, 33, 15);      // #7A8478
        let accent = Rgb565::new(20, 48, 16);     // #A7C080
        let focus_bg = Rgb565::new(7, 16, 8);     // #3A4246
        let danger = Rgb565::new(28, 31, 16);     // #E67E80
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
        let text = Rgb565::new(31, 62, 30);       // #F8F8F2
        let bg = Rgb565::new(4, 10, 4);           // #272822
        let muted = Rgb565::new(14, 28, 11);      // #75715E
        let accent = Rgb565::new(28, 54, 14);     // #E6DB74
        let focus_bg = Rgb565::new(9, 18, 7);     // #49483E
        let danger = Rgb565::new(31, 9, 14);      // #F92672
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
}

impl<'a, D, C> ColorGraphicsTarget<'a, D, C>
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
    ColorTheme<C>: Default,
{
    /// Creates a target using the default [`ColorTheme`] for `C`.
    pub fn new(display: &'a mut D, font: MonoFont<'static>) -> Self {
        Self { display, font, theme: ColorTheme::default() }
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
        Rectangle::new(self.px_point(area.x, area.y), Size::new(area.w as u32, area.h as u32))
    }

    /// Access to the underlying `DrawTarget` for pixel-level drawing.
    pub fn display_mut(&mut self) -> &mut D {
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

    /// Render `text` with its top-left at pixel `(x, y)`, in the theme's
    /// `(fg, bg)` for `style`. The background colour is always set so the whole
    /// cell repaints (required on a pixel display).
    fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style) {
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
    /// `Thick` 2px, `Double` two concentric rectangles).
    fn draw_box(&mut self, area: Area, border: BorderStyle) {
        if matches!(border, BorderStyle::None) {
            return;
        }
        let rect = self.px_rect(area);
        let top_left = rect.top_left;
        let size = rect.size;
        let stroke_color = self.theme.foreground(Style::Normal);

        match border {
            BorderStyle::None => unreachable!(),

            BorderStyle::Single => {
                let s = PrimitiveStyle::with_stroke(stroke_color, 1);
                let _ = Rectangle::new(top_left, size).into_styled(s).draw(self.display);
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
                let s = PrimitiveStyle::with_stroke(stroke_color, 2);
                let _ = Rectangle::new(top_left, size).into_styled(s).draw(self.display);
            }

            BorderStyle::Double => {
                let s = PrimitiveStyle::with_stroke(stroke_color, 1);
                let _ = Rectangle::new(top_left, size).into_styled(s).draw(self.display);
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
        let rect = self.px_rect(area);
        let s = PrimitiveStyle::with_fill(self.theme.background(Style::Normal));
        let _ = rect.into_styled(s).draw(self.display);
    }

    /// Fill the pixel region solid in the `style`'s foreground colour.
    fn fill_rect(&mut self, area: Area, style: Style) {
        let rect = self.px_rect(area);
        let s = PrimitiveStyle::with_fill(self.theme.foreground(style));
        let _ = rect.into_styled(s).draw(self.display);
    }

    /// A smooth Charm-style bar: a dark-grey rounded track with a rounded fill in
    /// the `style`'s colour (e.g. purple for `Accent`, lilac for `Focus`), filled
    /// to `fill_permille/1000`.
    fn draw_bar(&mut self, area: Area, fill_permille: u16, style: Style) {
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
        let t = ColorTheme::<Rgb565>::new()
            .with_colors(Style::Accent, Rgb565::WHITE, Rgb565::GREEN);
        assert_eq!(t.resolve(Style::Accent), (Rgb565::WHITE, Rgb565::GREEN));
        assert_eq!(t.resolve(Style::Normal), (Rgb565::new(31, 62, 31), Rgb565::new(3, 6, 3)));
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
}
