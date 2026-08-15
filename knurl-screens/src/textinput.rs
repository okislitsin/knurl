//! A [`TextInput`] in a form: the field that grows a second row while it is
//! being edited, and captures `Select` for its own token ribbon.
//!
//! The half-typed name does not survive leaving the screen, which is what
//! [`Screen::on_enter`] is for.

use knurl::{
    Area, Button, FocusChain, FocusZone, Form, FormField, Msg, Outcome, RenderTarget, Screen,
    ScreenState, TextInput,
};

use crate::{AppEvent, Panel};

pub struct TextInputScreen {
    state: ScreenState,
    form: Form,
    name: TextInput<'static, 16>,
    back: Button<'static>,
}

impl TextInputScreen {
    pub fn new(panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            form: Form::new(),
            name: TextInput::new("Name").with_label_width(panel.label_w),
            back: Button::new("< Back"),
        }
    }

    fn parts(&mut self) -> (&mut ScreenState, &mut Form, [&mut dyn FormField; 2]) {
        let Self {
            state,
            form,
            name,
            back,
        } = self;
        (state, form, [name, back])
    }
}

impl Screen for TextInputScreen {
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

    fn on_enter(&mut self) {
        self.name.reset();
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let (_, form, fields) = self.parts();
        form.view(target, area, &fields);
    }
}
