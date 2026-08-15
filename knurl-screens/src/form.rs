//! One form, the plainest composition there is - the baseline the other three
//! ([`two_forms`](crate::two_forms), [`tab_forms`](crate::tab_forms),
//! [`list_form`](crate::list_form)) are variations on.

use knurl::{
    Area, Button, Counter, FocusChain, FocusZone, Form, FormField, Msg, Outcome, RenderTarget,
    Screen, ScreenState, Toggle,
};

use crate::AppEvent;

pub struct FormScreen {
    state: ScreenState,
    form: Form,
    fan: Toggle<'static>,
    level: Counter<'static>,
    back: Button<'static>,
}

impl FormScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            form: Form::new(),
            fan: Toggle::new("Fan"),
            level: Counter::new("Lvl").with_range(0, 5).with_value(2),
            back: Button::new("< Back"),
        }
    }

    fn parts(&mut self) -> (&mut ScreenState, &mut Form, [&mut dyn FormField; 3]) {
        let Self {
            state,
            form,
            fan,
            level,
            back,
        } = self;
        (state, form, [fan, level, back])
    }
}

impl Default for FormScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for FormScreen {
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
