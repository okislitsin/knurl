//! A [`Radio`] group and a way out.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Msg, Outcome, Radio, RenderTarget, Screen, ScreenState, VStack,
};

use crate::AppEvent;

const OPTIONS: &[&str] = &["Low", "Medium", "High"];

pub struct RadioScreen {
    state: ScreenState,
    radio: Radio<'static>,
    back: Button<'static>,
}

impl RadioScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            radio: Radio::new(OPTIONS),
            back: Button::new("< Back"),
        }
    }
}

impl Default for RadioScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for RadioScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, radio, back } = self;
        f(state.chain(), &mut [radio, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.radio.view(target, body);
        self.back.view(target, foot);
    }
}
