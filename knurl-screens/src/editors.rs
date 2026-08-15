//! The value editors: a [`Counter`], a [`Slider`] and a [`Picker`] in one form.
//!
//! `Select` here enters an edit rather than acting, and while it runs the form
//! keeps the focus (a counter at its bound reports the event unspent, and the
//! cursor must not wander off mid-edit). None of that is screen code.

use knurl::{
    Area, Button, Counter, FocusChain, FocusZone, Form, FormField, Msg, Outcome, Picker,
    RenderTarget, Screen, ScreenState, Slider,
};

use crate::{AppEvent, Panel};

const MODES: &[&str] = &["Eco", "Balanced", "Turbo"];

pub struct EditorsScreen {
    state: ScreenState,
    form: Form,
    brightness: Counter<'static>,
    volume: Slider<'static>,
    mode: Picker<'static>,
    back: Button<'static>,
}

impl EditorsScreen {
    pub fn new(panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            form: Form::new(),
            brightness: Counter::new("Bright")
                .with_range(0, 100)
                .with_step(10)
                .with_value(60),
            volume: Slider::new("Vol")
                .with_range(0, 100)
                .with_step(10)
                .with_value(40)
                .with_label_width(panel.label_w),
            mode: Picker::new("Mode", MODES),
            back: Button::new("< Back"),
        }
    }

    fn parts(&mut self) -> (&mut ScreenState, &mut Form, [&mut dyn FormField; 4]) {
        let Self {
            state,
            form,
            brightness,
            volume,
            mode,
            back,
        } = self;
        (state, form, [brightness, volume, mode, back])
    }
}

impl Screen for EditorsScreen {
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
