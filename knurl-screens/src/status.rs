//! The static page: a [`StatusBar`] readout with nothing on it the cursor can
//! use, and a way out.
//!
//! Its zone list is one button. The bar itself is a [`NoZone`] - named in the
//! list because the screen does show it, skipped by the focus because there is
//! nothing there to drive.

use knurl::{
    Align, Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Msg, NoZone, Outcome, RenderTarget, Screen, ScreenState, StatusBar,
    Style, Title, VStack,
};

use crate::AppEvent;

pub struct StatusScreen {
    state: ScreenState,
    bar: NoZone,
    back: Button<'static>,
}

impl StatusScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            bar: NoZone,
            back: Button::new("< Back"),
        }
    }
}

impl Default for StatusScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for StatusScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, bar, back } = self;
        f(state.chain(), &mut [bar, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [bar, rest] = VStack::split(area, &[Length(lh), Fill(1)]);
        let [caption, foot] = VStack::split(rest, &[Fill(1), Length(lh)]);
        StatusBar::new()
            .with_left("L")
            .with_center("CTR")
            .with_right("R")
            .view(target, bar);
        Title::new("left / center / right")
            .with_align(Align::Center)
            .with_style(Style::Muted)
            .view(target, Area::new(caption.x, caption.y + lh, caption.w, lh));
        self.back.view(target, foot);
    }
}
