//! The root menu: one [`List`], and nothing else.
//!
//! The screen a trait like this has to keep cheap. It owns a single widget, so
//! it declares a single zone; there is no form here, no "< Back" (the root is
//! where Back would go *to*), and no ceremony.

use knurl::{
    Area, Component, FocusChain, FocusZone, List, Msg, Outcome, RenderTarget, Screen, ScreenState,
};

use crate::{AppEvent, MENU, Page};

pub struct MenuScreen {
    state: ScreenState,
    items: List<'static>,
}

impl MenuScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            items: List::new(MENU),
        }
    }
}

impl Default for MenuScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for MenuScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, items } = self;
        f(state.chain(), &mut [items]);
    }

    /// A chosen row opens its page; the last one leaves the demo.
    fn on_outcome(&mut self, _msg: &Msg, outcome: Outcome) -> Option<AppEvent> {
        if outcome != Outcome::Activated {
            return None;
        }
        Some(match Page::from_menu_row(self.items.selected()) {
            Some(page) => AppEvent::Open(page),
            None => AppEvent::Quit,
        })
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        self.items.view(target, area);
    }
}
