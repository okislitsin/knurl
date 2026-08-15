//! Text styles, drawn by hand and scrolled by hand - the screen with no widget
//! to put on the chain.
//!
//! Its rows are transient (`Label`s and `Title`s built per frame), so what the
//! encoder actually drives is an offset. That offset joins the chain as a
//! [`ScrollZone`], and everything else follows: the window scrolls while it
//! can, and at the bottom the same `Down` moves the cursor onto `< Back`.

use knurl::{
    Align, Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Label, Msg, Outcome, RenderTarget, Screen, ScreenState, ScrollZone,
    Separator, Style, Title, VStack,
};

use crate::{AppEvent, stack};

/// One row of the stack. A row is drawn from scratch every frame - which is
/// exactly why the screen clears the window itself (see [`stack::rows`]).
enum Row {
    Text(&'static str, Style),
    Centered(&'static str),
    Right(&'static str),
    Rule,
    Gap,
}

const ROWS: &[Row] = &[
    Row::Text("Normal text", Style::Normal),
    Row::Text("Accent text", Style::Accent),
    Row::Text("Muted text", Style::Muted),
    Row::Text("Danger text", Style::Danger),
    Row::Rule,
    Row::Centered("Centered title"),
    Row::Right("Right title"),
    Row::Gap,
    Row::Text("(spacer above)", Style::Muted),
];

pub struct TextScreen {
    state: ScreenState,
    scroll: usize,
    /// Rows that fitted last frame - the ceiling the scroll zone stops at.
    visible: usize,
    back: Button<'static>,
}

impl TextScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            scroll: 0,
            visible: 1,
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
            state,
            scroll,
            visible,
            back,
        } = self;
        let mut rows = ScrollZone::new(scroll, ROWS.len().saturating_sub(*visible));
        f(state.chain(), &mut [&mut rows, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        self.scroll = 0;
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [window, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.visible = stack::rows(
            target,
            window,
            self.scroll,
            ROWS.len(),
            |t, i, row| match &ROWS[i] {
                Row::Text(s, style) => Label::new(s).with_style(*style).view(t, row),
                Row::Centered(s) => Title::new(s).with_align(Align::Center).view(t, row),
                Row::Right(s) => Title::new(s).with_align(Align::Right).view(t, row),
                Row::Rule => Separator::new().view(t, row),
                Row::Gap => {}
            },
        );
        self.back.view(target, foot);
    }
}
