//! Position: a window over a fixed set of rows, with a [`Scrollbar`](knurl::Scrollbar)
//! beside it and a [`Paginator`] under it saying where in the whole the window
//! sits.
//!
//! Same shape as [`text`](crate::text) - a hand-drawn stack driven by a
//! [`ScrollZone`](knurl::ScrollZone) inside [`Stack`] - with a paginator added
//! underneath. The paginator is a field, so it repaints when the window moves
//! and not otherwise; its page count follows the panel, which is why it is set
//! rather than built in.
//!
//! The rows are built where they are drawn, and this is the one shape in which
//! that is honest: the window clears and repaints every row **only on the
//! frames where it shifted**, so a `Label` built there is dirty at the moment
//! its row genuinely changed. On every other frame nothing is built and nothing
//! is drawn - the screen used to report its whole body on every frame, idle
//! ones included.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, Msg, Outcome, Paginator, RenderTarget, Screen, ScreenState,
    Style, VStack,
};

use crate::{AppEvent, stack::Stack};

const ROWS: &[&str] = &[
    "Row 1", "Row 2", "Row 3", "Row 4", "Row 5", "Row 6", "Row 7", "Row 8", "Row 9", "Row 10",
    "Row 11", "Row 12",
];

pub struct PositionScreen {
    state: ScreenState,
    window: Stack,
    pages: Paginator,
    back: Button<'static>,
}

impl PositionScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            window: Stack::new(),
            // Corrected on the first frame, once the panel has said how many
            // rows fit.
            pages: Paginator::new(ROWS.len()),
            back: Button::new("< Back"),
        }
    }
}

impl Default for PositionScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for PositionScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self {
            state,
            window,
            back,
            ..
        } = self;
        let mut scroll = window.zone(ROWS.len());
        f(state.chain(), &mut [&mut scroll, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    /// Rows inside a hand-drawn window are not zones, so the cascade that
    /// repaints a screen on entry does not reach them - the window is told.
    fn on_enter(&mut self) {
        self.window.mark_dirty();
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let [rows, pages] = VStack::split(body, &[Fill(1), Length(lh)]);
        let Self {
            window,
            pages: paginator,
            back,
            ..
        } = self;

        // Which row carries the cursor depends on where the window is, not on
        // what is in it - so it is decided here, on the frames that shift.
        let top = window.scroll();
        window.rows(target, rows, ROWS.len(), |t, i, row, shifted| {
            if !shifted {
                return;
            }
            let style = if i == top { Style::Focus } else { Style::Muted };
            Label::new(ROWS[i]).with_style(style).view(t, row);
        });

        paginator.set_pages(ROWS.len().saturating_sub(window.visible()) + 1);
        paginator.set_current(window.scroll());
        paginator.view(target, pages);
        back.view(target, foot);
    }
}
