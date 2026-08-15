//! Position: a window over a fixed set of rows, with a [`Scrollbar`] beside it
//! and a [`Paginator`] under it saying where in the whole the window sits.
//!
//! Same shape as [`text`](crate::text) - a hand-drawn stack driven by a
//! [`ScrollZone`] - but it lays its own window out, because the scrollbar and
//! the paginator both have to agree with the row loop.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, Msg, Outcome, Paginator, RenderTarget, Screen, ScreenState,
    ScrollZone, Scrollbar, Style, VStack,
};

use crate::AppEvent;

const ROWS: &[&str] = &[
    "Row 1", "Row 2", "Row 3", "Row 4", "Row 5", "Row 6", "Row 7", "Row 8", "Row 9", "Row 10",
    "Row 11", "Row 12",
];

pub struct PositionScreen {
    state: ScreenState,
    offset: usize,
    visible: usize,
    back: Button<'static>,
}

impl PositionScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            offset: 0,
            visible: 1,
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
            offset,
            visible,
            back,
        } = self;
        let mut rows = ScrollZone::new(offset, ROWS.len().saturating_sub(*visible));
        f(state.chain(), &mut [&mut rows, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        self.offset = 0;
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let [rows, pages] = VStack::split(body, &[Fill(1), Length(lh)]);
        target.clear(rows);

        let visible = ((rows.h / lh) as usize).clamp(1, ROWS.len());
        self.visible = visible;
        for r in 0..visible {
            let Some(text) = ROWS.get(self.offset + r) else {
                break;
            };
            let style = if r == 0 { Style::Focus } else { Style::Muted };
            Label::new(text).with_style(style).view(
                target,
                Area::new(rows.x, rows.y + r as u16 * lh, rows.w.saturating_sub(4), lh),
            );
        }

        let mut bar = Scrollbar::new();
        bar.set(ROWS.len(), visible, self.offset);
        bar.view(
            target,
            Area::new(
                rows.x + rows.w.saturating_sub(3),
                rows.y,
                3,
                visible as u16 * lh,
            ),
        );
        Paginator::new(ROWS.len() - visible + 1)
            .with_current(self.offset)
            .view(target, pages);
        self.back.view(target, foot);
    }
}
