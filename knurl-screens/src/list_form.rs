//! **A list and a form on one screen** - two zones of different kinds sharing
//! the area and one cursor.
//!
//! The list picks a channel, the form edits it. `Down` off the last item of the
//! list steps into the form's first field; `Up` off that field steps back onto
//! the list's last item. The screen writes neither rule.
//!
//! `< Back` is its own zone here rather than a form field: the form is only
//! part of the screen, so a way out inside it would be a way out of the form.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    Counter, FocusChain, FocusZone, Form, FormField, List, Msg, Outcome, RenderTarget, Screen,
    ScreenState, Toggle, VStack,
};

use crate::AppEvent;

const CHANNELS: &[&str] = &["Ch 1", "Ch 2", "Ch 3", "Ch 4"];

pub struct ListFormScreen {
    state: ScreenState,
    channels: List<'static>,
    form: Form,
    enabled: Toggle<'static>,
    gain: Counter<'static>,
    back: Button<'static>,
}

impl ListFormScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            channels: List::new(CHANNELS),
            form: Form::new(),
            enabled: Toggle::new("On").with_on(true),
            gain: Counter::new("Gain").with_range(0, 9).with_value(3),
            back: Button::new("< Back"),
        }
    }

    #[allow(clippy::type_complexity)]
    fn parts(
        &mut self,
    ) -> (
        &mut ScreenState,
        &mut List<'static>,
        &mut Form,
        [&mut dyn FormField; 2],
        &mut Button<'static>,
    ) {
        let Self {
            state,
            channels,
            form,
            enabled,
            gain,
            back,
        } = self;
        (state, channels, form, [enabled, gain], back)
    }
}

impl Default for ListFormScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for ListFormScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let (state, channels, form, mut fields, back) = self.parts();
        let mut zone = form.zone(&mut fields);
        f(state.chain(), &mut [channels, &mut zone, back]);
    }

    /// Choosing a channel is an activation too - and not the way out, which is
    /// why the button is asked rather than the focus index consulted.
    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        // The form needs two rows; the list takes what is left.
        let [rows, fields_area] = VStack::split(body, &[Fill(1), Length(lh * 2)]);
        let (_, channels, form, fields, back) = self.parts();
        channels.view(target, rows);
        form.view(target, fields_area, &fields);
        back.view(target, foot);
    }
}
