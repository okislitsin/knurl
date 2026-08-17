//! A widget written from outside the library, by the contract in
//! [`knurl::custom_widget`], and the screen that runs it.
//!
//! Nothing here is privileged: this file sees exactly what a firmware project
//! sees - the `knurl` facade and nothing else - so [`Thermostat`] is a worked
//! answer to "what does writing my own actually take".
//!
//! It is the case a [`Canvas`](knurl::Canvas) does not cover: a picture the
//! user *drives*. A canvas brings the four obligations and takes a closure, but
//! a closure has no state and refuses the focus. A setpoint has both, so it
//! needs a type - and once it is a type, the whole contract applies:
//!
//! - a `Cell<bool>` dirty flag set in `update` **only on a real change**;
//! - a zero-area guard, because `draw` gets whatever area the screen has;
//! - an [`Outcome`] for every event: `Consumed` while the dial has room,
//!   `Ignored` at either limit so the focus chain moves the cursor on, and
//!   `Activated` on `Select` - even when applying changes not one pixel;
//! - [`Style`], never a colour, so it renders on a mono OLED, on a colour TFT
//!   and against a recording target in a test.

use core::cell::Cell;

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, Msg, Outcome, RenderTarget, Screen, ScreenState, Style, VStack,
};

use crate::AppEvent;

// ── The widget ───────────────────────────────────────────────────────────────

/// At and above this, the setpoint is drawn as a warning rather than a value -
/// the widget's one opinion, and it is expressed as a [`Style`], not a colour.
const WARM_C: i16 = 26;

/// Formats `v` (0..=99) as `"22C"` into a stack buffer - no allocation, ASCII.
fn degrees(buf: &mut [u8; 3], v: i16) -> &str {
    let v = v.clamp(0, 99) as u8;
    buf[0] = b'0' + v / 10;
    buf[1] = b'0' + v % 10;
    buf[2] = b'C';
    core::str::from_utf8(buf).unwrap_or("??C")
}

/// A setpoint dial: a temperature the encoder turns, a bar showing where it
/// sits between the limits, and a mark at the value that was last applied.
///
/// Rotating moves the setpoint; pressing applies it. The mark is what makes the
/// difference visible - dialling away from it and pressing again is the whole
/// interaction, and it is why `Select` has to report
/// [`Activated`](Outcome::Activated): the application is what does something
/// about a new setpoint.
pub struct Thermostat {
    setpoint: i16,
    /// The setpoint the application last acted on - drawn as a tick.
    applied: i16,
    focused: bool,
    /// Repaint gate: set when the setpoint, the applied mark or focus actually
    /// changes. Starts dirty so the first frame draws.
    dirty: Cell<bool>,
}

impl Thermostat {
    /// The coldest and warmest the dial goes, in whole degrees. Public because
    /// the limits are where the widget hands events back, and a test that
    /// cannot name them has to guess.
    pub const MIN_C: i16 = 5;
    pub const MAX_C: i16 = 30;

    pub const fn new(setpoint: i16) -> Self {
        Self {
            setpoint,
            applied: setpoint,
            focused: false,
            dirty: Cell::new(true),
        }
    }

    /// The temperature currently dialled in.
    pub fn setpoint(&self) -> i16 {
        self.setpoint
    }

    /// The temperature the application last applied.
    pub fn applied(&self) -> i16 {
        self.applied
    }

    /// How far along the scale the setpoint sits, in permille.
    fn permille(&self, v: i16) -> u16 {
        let span = (Self::MAX_C - Self::MIN_C).max(1) as i32;
        ((v.clamp(Self::MIN_C, Self::MAX_C) - Self::MIN_C) as i32 * 1000 / span) as u16
    }
}

impl Component for Thermostat {
    fn update(&mut self, msg: &Msg) -> Outcome {
        match msg {
            // A real change, and only then the flag.
            Msg::Up if self.setpoint < Self::MAX_C => {
                self.setpoint += 1;
                self.dirty.set(true);
                Outcome::Consumed
            }
            Msg::Down if self.setpoint > Self::MIN_C => {
                self.setpoint -= 1;
                self.dirty.set(true);
                Outcome::Consumed
            }
            // Applying is what the application acts on, so it is `Activated`
            // whether or not it moves a pixel: pressing on an already-applied
            // setpoint repaints nothing and is still the user's decision. An
            // outcome describes the event, never the picture.
            Msg::Select => {
                if self.applied != self.setpoint {
                    self.applied = self.setpoint;
                    self.dirty.set(true);
                }
                Outcome::Activated
            }
            // Either limit, or an event that was never ours (a `Tick`, a
            // `Char`): unspent, so the container may give it to somebody else.
            // This is what lets the cursor leave the dial for `< Back`.
            _ => Outcome::Ignored,
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        // `view` guarantees a non-empty area, never a *useful* one: a screen may
        // hand over four pixels, and a widget that indexes into that panics.
        if area.w < 4 * target.char_width() || area.h < lh {
            return;
        }

        // The value row. A band across the whole row is the focus language every
        // other widget speaks - and on a monochrome panel it is the only cue
        // that survives, since a mono target maps most styles to the same ink.
        if self.focused {
            target.fill_band(Area::new(area.x, area.y, area.w, lh), Style::Focus);
        }
        let value_style = match (self.focused, self.setpoint >= WARM_C) {
            (_, true) => Style::Danger,
            (true, false) => Style::Focus,
            (false, false) => Style::Normal,
        };
        target.draw_text(area.x, area.y, "Set", Style::Muted);
        let mut buf = [0u8; 3];
        let text = degrees(&mut buf, self.setpoint);
        let tw = target.text_width(text);
        target.draw_text(area.x + area.w.saturating_sub(tw), area.y, text, value_style);

        // The scale, if the screen gave us a second row for it.
        if area.h < lh * 2 {
            return;
        }
        let track = Area::new(area.x, area.y + lh + 1, area.w, lh.saturating_sub(2));
        target.draw_rect(track, Style::Muted);
        let Some(inner) = track.inner() else { return };
        let fill_w = (inner.w as u32 * self.permille(self.setpoint) as u32 / 1000) as u16;
        if fill_w > 0 {
            target.fill_rect(
                Area::new(inner.x, inner.y, fill_w, inner.h),
                if self.setpoint >= WARM_C {
                    Style::Danger
                } else {
                    Style::Accent
                },
            );
        }
        // Where the application last agreed with the dial.
        let mark_x = inner.x + (inner.w as u32 * self.permille(self.applied) as u32 / 1000) as u16;
        let mark_x = mark_x.min(inner.x + inner.w.saturating_sub(1));
        target.draw_line(
            mark_x,
            track.y.saturating_sub(1),
            mark_x,
            track.y + track.h,
            Style::Accent,
        );
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

// ── The screen that runs it ──────────────────────────────────────────────────

/// The dial, a hint, and the way out. The dial is a zone like any other: the
/// blanket [`FocusZone`] implementation covers every [`Component`], so nothing
/// here says it is one.
pub struct ThermostatScreen {
    state: ScreenState,
    caption: Label<'static>,
    dial: Thermostat,
    hint: Label<'static>,
    back: Button<'static>,
}

impl ThermostatScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            caption: Label::new("Target temperature").with_style(Style::Muted),
            dial: Thermostat::new(20),
            hint: Label::new("Push: apply").with_style(Style::Muted),
            back: Button::new("< Back"),
        }
    }
}

impl Default for ThermostatScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for ThermostatScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self {
            state, dial, back, ..
        } = self;
        f(state.chain(), &mut [dial, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        // Applying is the dial's own business - it draws its mark and there is
        // no hardware behind it here - so only the way out is an app event.
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [caption, dial, hint, foot] = VStack::split(
            area,
            &[Length(lh), Length(lh * 2 + 2), Fill(1), Length(lh)],
        );
        self.caption.view(target, caption);
        self.dial.view(target, dial);
        self.hint.view(target, Area::new(hint.x, hint.y, hint.w, lh));
        self.back.view(target, foot);
    }
}
