//! A form of two momentary fields, `< Back` among them.
//!
//! The convention for a form screen: `< Back` is the form's **last field**, so
//! it scrolls with the rest instead of taking a permanent row off a panel that
//! is already short. Pressing it is not deduced from that position - the button
//! is asked.

use knurl::{
    Area, Button, Checkbox, FocusChain, FocusZone, Form, FormField, Msg, Outcome, RenderTarget,
    Screen, ScreenState, Toggle,
};

use crate::AppEvent;

pub struct TogglesScreen {
    state: ScreenState,
    form: Form,
    logging: Checkbox<'static>,
    wifi: Toggle<'static>,
    back: Button<'static>,
}

impl TogglesScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            form: Form::new(),
            logging: Checkbox::new("Logging"),
            wifi: Toggle::new("Wi-Fi").with_on(true),
            back: Button::new("< Back"),
        }
    }

    /// The screen's fields, named once: `zones` routes this array and `draw`
    /// paints it, so there is no second list to keep in step.
    fn parts(&mut self) -> (&mut ScreenState, &mut Form, [&mut dyn FormField; 3]) {
        let Self {
            state,
            form,
            logging,
            wifi,
            back,
        } = self;
        (state, form, [logging, wifi, back])
    }
}

impl Default for TogglesScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for TogglesScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let (state, form, mut fields) = self.parts();
        let mut zone = form.zone(&mut fields);
        f(state.chain(), &mut [&mut zone]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let (_, form, fields) = self.parts();
        form.view(target, area, &fields);
    }
}
