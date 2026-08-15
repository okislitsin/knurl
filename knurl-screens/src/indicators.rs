//! Live indicators: a [`Spinner`], a [`ProgressBar`] and a [`LineGauge`] over a
//! scrolled stack.
//!
//! The screen that shows why animation does not go through the focus chain: the
//! cursor is on the scroll window (or on `< Back`), and the indicators still
//! have to move. They are advanced in [`Screen::tick`], which the application
//! calls directly.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, LineGauge, Msg, Outcome, ProgressBar, RenderTarget, Screen,
    ScreenState, ScrollZone, Spinner, Style, VStack,
};

use crate::{AppEvent, stack};

/// Rows: spinner, caption, bar, caption, gauge, caption.
const ROW_COUNT: usize = 6;

/// A 0..=100 triangle wave, so the indicators sweep instead of jumping.
fn triangle(phase: u32) -> u16 {
    let v = (phase / 2) % 200;
    (if v < 100 { v } else { 200 - v }) as u16
}

pub struct IndicatorsScreen {
    state: ScreenState,
    spinner: Spinner,
    phase: u32,
    level: u16,
    scroll: usize,
    visible: usize,
    back: Button<'static>,
}

impl IndicatorsScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            spinner: Spinner::new().with_label("live"),
            phase: 0,
            level: 0,
            scroll: 0,
            visible: 1,
            back: Button::new("< Back"),
        }
    }
}

impl Default for IndicatorsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for IndicatorsScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self {
            state,
            scroll,
            visible,
            back,
            ..
        } = self;
        let mut rows = ScrollZone::new(scroll, ROW_COUNT.saturating_sub(*visible));
        f(state.chain(), &mut [&mut rows, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        self.scroll = 0;
        self.spinner.mark_dirty(); // drawn inside the stack, not gated by it
    }

    /// Past the chain, on purpose: the encoder is somewhere else entirely.
    fn tick(&mut self) -> bool {
        self.phase = self.phase.wrapping_add(1);
        self.level = triangle(self.phase);
        let _ = self.spinner.update(&Msg::Tick);
        true
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [window, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let (spinner, level) = (&self.spinner, self.level);
        self.visible = stack::rows(
            target,
            window,
            self.scroll,
            ROW_COUNT,
            |t, i, row| match i {
                0 => spinner.view(t, row),
                1 => Label::new("Progress").with_style(Style::Muted).view(t, row),
                2 => {
                    let mut bar = ProgressBar::new().with_max(100);
                    bar.set_value(level);
                    bar.view(t, row);
                }
                3 => Label::new("Gauge").with_style(Style::Muted).view(t, row),
                4 => {
                    let mut gauge = LineGauge::new().with_max(100);
                    gauge.set_value(level);
                    gauge.view(t, row);
                }
                _ => Label::new("live values")
                    .with_style(Style::Muted)
                    .view(t, row),
            },
        );
        self.back.view(target, foot);
    }
}
