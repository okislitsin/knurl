//! A [`Pager`] over any [`LinesModel`] and a way out.
//!
//! Generic over the model on purpose: on a device the lines are a `const`
//! array; in the TFT demo they are a live ring buffer that grows under the
//! screen, and the screen does not change a line either way. `tick` is what
//! keeps a following pager pinned to the newest line.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, LinesModel, Msg, Outcome, Pager, RenderTarget, Screen, ScreenState,
    VStack,
};

use crate::AppEvent;

pub struct PagerScreen<'a, M: LinesModel + ?Sized> {
    state: ScreenState,
    pager: Pager<'a, M>,
    back: Button<'static>,
}

impl<'a, M: LinesModel + ?Sized> PagerScreen<'a, M> {
    pub fn new(lines: &'a M) -> Self {
        Self {
            state: ScreenState::new(),
            pager: Pager::new(lines),
            back: Button::new("< Back"),
        }
    }

    /// Follow mode: the view stays pinned to the newest line until the user
    /// scrolls away from the bottom.
    pub fn with_follow(mut self, on: bool) -> Self {
        self.pager = self.pager.with_follow(on);
        self
    }
}

impl<M: LinesModel + ?Sized> Screen for PagerScreen<'_, M> {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, pager, back } = self;
        f(state.chain(), &mut [pager, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    /// A growing log moves under a following pager without any input at all.
    fn tick(&mut self) -> bool {
        self.pager.update(&Msg::Tick).is_handled()
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.pager.view(target, body);
        self.back.view(target, foot);
    }
}
