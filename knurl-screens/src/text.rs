//! Text styles, drawn by hand and scrolled by hand - the screen with no widget
//! to put on the chain.
//!
//! What the encoder actually drives here is an offset. That offset joins the
//! chain as a [`ScrollZone`](knurl::ScrollZone) inside [`Stack`], and everything
//! else follows: the window scrolls while it can, and at the bottom the same
//! `Down` moves the cursor onto `< Back`.
//!
//! The rows are **fields**, not values built inside `draw`. Nine static rows
//! rebuilt per frame look identical on the panel and report the whole window
//! dirty forever; kept in an array they paint once and the screen goes quiet.

use knurl::{
    Align, Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, Msg, Outcome, RenderTarget, Screen, ScreenState, Separator,
    Style, Title, VStack,
};

use crate::{AppEvent, stack::Stack};

/// One row of the stack. Rows differ in type, so they share an enum - which is
/// what keeping them in fields costs, and it is cheaper than the repaint.
enum Row {
    Text(Label<'static>),
    Heading(Title<'static>),
    Rule(Separator),
    Gap,
}

impl Row {
    fn view(&self, target: &mut dyn RenderTarget, area: Area) {
        match self {
            Row::Text(w) => w.view(target, area),
            Row::Heading(w) => w.view(target, area),
            Row::Rule(w) => w.view(target, area),
            Row::Gap => {}
        }
    }

    fn mark_dirty(&self) {
        match self {
            Row::Text(w) => w.mark_dirty(),
            Row::Heading(w) => w.mark_dirty(),
            Row::Rule(w) => w.mark_dirty(),
            Row::Gap => {}
        }
    }
}

const ROW_COUNT: usize = 9;

fn rows() -> [Row; ROW_COUNT] {
    [
        Row::Text(Label::new("Normal text")),
        Row::Text(Label::new("Accent text").with_style(Style::Accent)),
        Row::Text(Label::new("Muted text").with_style(Style::Muted)),
        Row::Text(Label::new("Danger text").with_style(Style::Danger)),
        Row::Rule(Separator::new()),
        Row::Heading(Title::new("Centered title").with_align(Align::Center)),
        Row::Heading(Title::new("Right title").with_align(Align::Right)),
        Row::Gap,
        Row::Text(Label::new("(spacer above)").with_style(Style::Muted)),
    ]
}

pub struct TextScreen {
    state: ScreenState,
    window: Stack,
    rows: [Row; ROW_COUNT],
    back: Button<'static>,
}

impl TextScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            window: Stack::new(),
            rows: rows(),
            back: Button::new("< Back"),
        }
    }
}

impl Default for TextScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for TextScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self {
            state, window, back, ..
        } = self;
        let mut scroll = window.zone(ROW_COUNT);
        f(state.chain(), &mut [&mut scroll, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        // Rows inside a hand-drawn window are not zones, so the cascade that
        // repaints a screen on entry does not reach them - the window is told.
        self.window.mark_dirty();
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let Self {
            window, rows, back, ..
        } = self;

        window.rows(target, body, ROW_COUNT, |t, i, area, shifted| {
            if shifted {
                rows[i].mark_dirty();
            }
            rows[i].view(t, area);
        });
        back.view(target, foot);
    }
}
