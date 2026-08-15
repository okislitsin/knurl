//! A [`Table`] and a way out. Column widths are pixels, so they are the one
//! thing the screen takes from its [`Panel`](crate::Panel).

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Msg, Outcome, RenderTarget, Screen, ScreenState, Table, VStack,
};

use crate::{AppEvent, Panel};

const ROWS: [[&str; 3]; 5] = [
    ["Bolt", "12", "3"],
    ["Nut", "34", "1"],
    ["Washer", "90", "1"],
    ["Screw", "75", "4"],
    ["Rivet", "21", "2"],
];
const HEADERS: &[&str] = &["Name", "Qty", "Pri"];

pub struct TableScreen {
    state: ScreenState,
    table: Table<'static, [[&'static str; 3]; 5]>,
    back: Button<'static>,
}

impl TableScreen {
    pub fn new(panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            table: Table::new(&ROWS, panel.table_w).with_headers(HEADERS),
            back: Button::new("< Back"),
        }
    }
}

impl Screen for TableScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, table, back } = self;
        f(state.chain(), &mut [table, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.table.view(target, body);
        self.back.view(target, foot);
    }
}
