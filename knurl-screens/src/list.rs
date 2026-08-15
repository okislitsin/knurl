//! A [`List`] and a way out: the plainest two-zone screen there is.
//!
//! "`Down` past the last item lands on `< Back`" is not written here - the
//! chain does it, because the list reports an edge and the button is next.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, List, Msg, Outcome, RenderTarget, Screen, ScreenState, VStack,
};

use crate::AppEvent;

const ITEMS: &[&str] = &[
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet",
    "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
];

pub struct ListScreen {
    state: ScreenState,
    items: List<'static>,
    back: Button<'static>,
}

impl ListScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            items: List::new(ITEMS),
            back: Button::new("< Back"),
        }
    }
}

impl Default for ListScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for ListScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, items, back } = self;
        f(state.chain(), &mut [items, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.items.view(target, body);
        self.back.view(target, foot);
    }
}
