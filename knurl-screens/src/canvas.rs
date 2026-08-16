//! **Drawing of its own** - a needle gauge, a rolling sparkline and a 1-bit
//! icon, none of which is a widget in the library.
//!
//! Everything here is drawn through [`Canvas`] with the portable primitives
//! ([`draw_line`](knurl::RenderTarget::draw_line),
//! [`draw_rect`](knurl::RenderTarget::draw_rect),
//! [`set_pixel`](knurl::RenderTarget::set_pixel),
//! [`draw_bitmap`](knurl::RenderTarget::draw_bitmap)) and coloured by
//! [`Style`], never by an `Rgb565` - which is why the same file draws on a
//! mono OLED and on a colour TFT, and why it compiles for bare metal.
//!
//! It also shows both ways to run a canvas, because they answer the same
//! question - *a closure cannot say that it changed* - differently:
//!
//! - the gauge and the sparkline are **built in `draw`**, capturing the value
//!   they paint. A fresh canvas is dirty, so they repaint every frame, which is
//!   what a live readout wants anyway;
//! - the icon is **kept as a field** and paints once. Nothing about it changes,
//!   so nothing repaints it - until [`Screen::on_enter`] marks it dirty, which
//!   it must, because the screen's repaint cascade reaches zones and a canvas
//!   is not one.

use knurl::{
    Area, Button, Canvas, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, HStack, Msg, Outcome, RenderTarget, Screen, ScreenState, Style, VStack,
};

use crate::AppEvent;

/// Samples kept behind the sparkline. A plain array - no `alloc` here, and
/// none on a device either.
const SAMPLES: usize = 24;

/// Ticks between sparkline samples: the frame loop is far faster than anything
/// worth plotting.
const SAMPLE_EVERY: u32 = 8;

/// `(cos, sin) * 256` every 15 degrees, from 180 (needle hard left) to 0 (hard
/// right). A lookup table is the whole of "trigonometry" a dial needs, and it
/// keeps the screen integer-only - there is no FPU on the target.
const SWEEP: [(i32, i32); 13] = [
    (-256, 0),
    (-247, 66),
    (-222, 128),
    (-181, 181),
    (-128, 222),
    (-66, 247),
    (0, 256),
    (66, 247),
    (128, 222),
    (181, 181),
    (222, 128),
    (247, 66),
    (256, 0),
];

/// An 8x8 knurled knob, in the format
/// [`draw_bitmap`](knurl::RenderTarget::draw_bitmap) documents: one byte per
/// row, most significant bit leftmost, clear bits transparent.
const KNOB: [u8; 8] = [
    0b0011_1100,
    0b0111_1110,
    0b1110_0111,
    0b1100_0011,
    0b1100_0011,
    0b1110_0111,
    0b0111_1110,
    0b0011_1100,
];

/// The icon's side, in pixels - it is a sprite, so it has exactly one size.
const ICON_PX: u16 = 8;

// ── The three drawings ───────────────────────────────────────────────────────

/// A needle gauge: a frame, a tick every 15 degrees around the sweep, and the
/// needle at `level` (0..=100).
///
/// The dial sizes itself from the shorter side and is centred in `a`, so a
/// 320px panel gets a big dial rather than a stretched box - the screen is
/// panel-independent, and this is what that costs: three lines of arithmetic
/// instead of a hardcoded 128.
fn gauge(t: &mut dyn RenderTarget, a: Area, level: u16) {
    if a.w < 12 || a.h < 8 {
        return;
    }
    let r = i32::from((a.h - 3).min(a.w / 2 - 1));
    let dial = Area::new(
        a.x + (a.w - (2 * r + 1) as u16) / 2,
        a.y + a.h - (r + 3) as u16,
        (2 * r + 1) as u16,
        (r + 3) as u16,
    );
    t.draw_rect(dial, Style::Muted);

    let cx = i32::from(dial.x) + r;
    let cy = i32::from(dial.y + dial.h) - 2; // the pivot, a pixel off the frame
    let at = |radius: i32, (cos, sin): (i32, i32)| {
        (
            (cx + radius * cos / 256) as u16,
            (cy - radius * sin / 256) as u16,
        )
    };

    // A tick is one line, not a run of pixels - see the note on the primitives.
    let tick = (r / 6).max(3);
    for step in SWEEP {
        let (x0, y0) = at(r, step);
        let (x1, y1) = at(r - tick, step);
        t.draw_line(x0, y0, x1, y1, Style::Muted);
    }

    let step = (level.min(100) as usize * (SWEEP.len() - 1)) / 100;
    let (nx, ny) = at(r - tick - 1, SWEEP[step]);
    t.draw_line(cx as u16, cy as u16, nx, ny, Style::Accent);
    t.set_pixel(cx as u16, cy as u16, Style::Accent); // the hub
}

/// A sparkline of `samples` (each 0..=100) over a baseline, oldest on the left.
fn sparkline(t: &mut dyn RenderTarget, a: Area, samples: &[u16; SAMPLES]) {
    if a.w < 8 || a.h < 4 {
        return;
    }
    let (right, bottom) = (a.x + a.w - 1, a.y + a.h - 1);
    t.draw_line(a.x, bottom, right, bottom, Style::Muted);

    let span = u32::from(a.h - 2);
    let point = |i: usize| -> (u16, u16) {
        let x = a.x + (i as u32 * u32::from(a.w - 1) / (SAMPLES as u32 - 1)) as u16;
        let y = bottom - 1 - (u32::from(samples[i].min(100)) * span / 100) as u16;
        (x, y)
    };
    let mut prev = point(0);
    for i in 1..SAMPLES {
        let next = point(i);
        t.draw_line(prev.0, prev.1, next.0, next.1, Style::Accent);
        prev = next;
    }
}

/// The 1-bit knob sprite, centred in whatever cell it is given.
fn icon(t: &mut dyn RenderTarget, a: Area) {
    if a.w < ICON_PX || a.h < ICON_PX {
        return;
    }
    let cell = Area::new(a.x, a.y + (a.h - ICON_PX) / 2, ICON_PX, ICON_PX);
    t.draw_bitmap(cell, &KNOB, Style::Accent);
}

// ── The screen ───────────────────────────────────────────────────────────────

pub struct CanvasScreen {
    state: ScreenState,
    phase: u32,
    level: u16,
    samples: [u16; SAMPLES],
    /// A canvas kept across frames needs a nameable type, and a closure has
    /// none - a non-capturing one coerces to a plain `fn`.
    knob: Canvas<fn(&mut dyn RenderTarget, Area)>,
    back: Button<'static>,
}

impl CanvasScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            phase: 0,
            level: 0,
            samples: [0; SAMPLES],
            knob: Canvas::new(icon),
            back: Button::new("< Back"),
        }
    }
}

impl Default for CanvasScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// A 0..=100 triangle wave, so the needle sweeps instead of jumping.
fn triangle(phase: u32) -> u16 {
    let v = (phase / 2) % 200;
    (if v < 100 { v } else { 200 - v }) as u16
}

impl Screen for CanvasScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, back, .. } = self;
        f(state.chain(), &mut [back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        // The cascade behind `enter` walks the screen's zones, and a canvas is
        // not one - so the icon would come back to a cleared screen and paint
        // nothing, being perfectly clean.
        self.knob.mark_dirty();
    }

    fn tick(&mut self) -> bool {
        self.phase = self.phase.wrapping_add(1);
        self.level = triangle(self.phase);
        if self.phase.is_multiple_of(SAMPLE_EVERY) {
            // The needle's slow sweep plus a wave three times as fast: a
            // straight ramp would say nothing about whether the sparkline
            // works.
            let sample = (self.level + triangle(self.phase.wrapping_mul(3))) / 2;
            self.samples.rotate_left(1);
            self.samples[SAMPLES - 1] = sample;
        }
        true
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [dial, strip, foot] = VStack::split(area, &[Fill(2), Fill(1), Length(lh)]);
        let [badge, spark] = HStack::split(strip, &[Length(ICON_PX + 2), Fill(1)]);

        // Built here, capturing what they draw: dirty by construction, and the
        // values change every tick anyway.
        let level = self.level;
        Canvas::new(move |t: &mut dyn RenderTarget, a: Area| gauge(t, a, level)).view(target, dial);

        let samples = self.samples;
        Canvas::new(move |t: &mut dyn RenderTarget, a: Area| sparkline(t, a, &samples))
            .view(target, spark);

        // Kept across frames: paints once, then only when told.
        self.knob.view(target, badge);
        self.back.view(target, foot);
    }
}
