//! Live indicators: a [`Spinner`], a [`ProgressBar`] and a [`LineGauge`] over a
//! scrolled stack.
//!
//! The screen that shows why animation does not go through the focus chain: the
//! cursor is on the scroll window (or on `< Back`), and the indicators still
//! have to move. They are advanced in [`Screen::tick`], which the application
//! calls directly.
//!
//! It is also the screen where the cost of getting the dirty gate wrong was
//! measured. Every row lives in a field here - the two captions and the three
//! indicators - so a tick repaints the indicators that moved and nothing else.
//! Built inside `draw` instead, the same tick reported 84% of a 320x240 panel
//! dirty, because a widget built per frame is dirty by construction.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, LineGauge, Msg, Outcome, ProgressBar, RenderTarget, Screen,
    ScreenState, Spinner, Style, VStack,
};

use crate::{AppEvent, stack::Stack};

/// Rows: spinner, caption, bar, caption, gauge, caption.
const ROW_COUNT: usize = 6;

/// A 0..=100 triangle wave, so the indicators sweep instead of jumping.
fn triangle(phase: u32) -> u16 {
    let v = (phase / 2) % 200;
    (if v < 100 { v } else { 200 - v }) as u16
}

pub struct IndicatorsScreen {
    state: ScreenState,
    phase: u32,
    rows: Stack,
    spinner: Spinner,
    progress_caption: Label<'static>,
    bar: ProgressBar,
    gauge_caption: Label<'static>,
    gauge: LineGauge,
    footer: Label<'static>,
    back: Button<'static>,
}

impl IndicatorsScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            phase: 0,
            rows: Stack::new(),
            spinner: Spinner::new().with_label("live"),
            progress_caption: Label::new("Progress").with_style(Style::Muted),
            bar: ProgressBar::new().with_max(100),
            gauge_caption: Label::new("Gauge").with_style(Style::Muted),
            gauge: LineGauge::new().with_max(100),
            footer: Label::new("live values").with_style(Style::Muted),
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
            state, rows, back, ..
        } = self;
        let mut window = rows.zone(ROW_COUNT);
        f(state.chain(), &mut [&mut window, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        // The rows are inside a hand-drawn window, and the repaint cascade walks
        // zones - so the window is the one thing that has to be told itself.
        self.rows.mark_dirty();
    }

    /// Past the chain, on purpose: the encoder is somewhere else entirely.
    fn tick(&mut self) -> bool {
        self.phase = self.phase.wrapping_add(1);
        let level = triangle(self.phase);
        // Setters that dirty only on a real change: two ticks out of three the
        // level lands on the value it already had, and those cost nothing.
        self.bar.set_value(level);
        self.gauge.set_value(level);
        let _ = self.spinner.update(&Msg::Tick);
        true
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [window, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let Self {
            rows,
            spinner,
            progress_caption,
            bar,
            gauge_caption,
            gauge,
            footer,
            back,
            ..
        } = self;

        rows.rows(target, window, ROW_COUNT, |t, i, row, shifted| {
            // `shifted` means this slot now shows a different row than it did,
            // so whatever lands in it owes a paint. Otherwise every widget
            // answers for itself, and a still row draws nothing.
            let w: &dyn Component = match i {
                0 => spinner,
                1 => progress_caption,
                2 => bar,
                3 => gauge_caption,
                4 => gauge,
                _ => footer,
            };
            if shifted {
                w.mark_dirty();
            }
            w.view(t, row);
        });
        back.view(target, foot);
    }
}
