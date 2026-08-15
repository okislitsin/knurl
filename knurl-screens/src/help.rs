//! A [`Help`] table of the encoder's vocabulary, and a way out.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Help, Msg, Outcome, RenderTarget, Screen, ScreenState, VStack,
};

use crate::{AppEvent, Panel};

const ITEMS: &[(&str, &str)] = &[
    ("Turn", "Move / scroll"),
    ("Push", "Select / edit"),
    ("Back", "Menu item"),
    ("Exit", "Leave demo"),
    ("Edit", "Push a value"),
];

pub struct HelpScreen {
    state: ScreenState,
    help: Help<'static>,
    back: Button<'static>,
}

impl HelpScreen {
    pub fn new(panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            help: Help::new(ITEMS).with_key_width(panel.key_w),
            back: Button::new("< Back"),
        }
    }
}

impl Screen for HelpScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, help, back } = self;
        f(state.chain(), &mut [help, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.help.view(target, body);
        self.back.view(target, foot);
    }
}
