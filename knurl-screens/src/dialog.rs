//! A modal [`Dialog`] over a screen that also has a way out - two things that
//! both answer `Select` with an `Activated`, and are told apart by asking each
//! of them rather than by counting zones.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    Dialog, FocusChain, FocusZone, Msg, Outcome, RenderTarget, Screen, ScreenState, VStack,
};

use crate::AppEvent;

const BUTTONS: &[&str] = &["OK", "Cancel"];

pub struct DialogScreen {
    state: ScreenState,
    dialog: Dialog<'static>,
    back: Button<'static>,
}

impl DialogScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            dialog: Dialog::new("Save?", "Apply settings?", BUTTONS),
            back: Button::new("< Back"),
        }
    }
}

impl Default for DialogScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for DialogScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self {
            state,
            dialog,
            back,
        } = self;
        f(state.chain(), &mut [dialog, back]);
    }

    /// Either dialog button closes the modal - `selected_button()` says which,
    /// if the application cares. This one treats both as "done here".
    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        if self.dialog.take_confirmed() {
            return Some(AppEvent::GoBack);
        }
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.dialog.view(target, body);
        self.back.view(target, foot);
    }
}
