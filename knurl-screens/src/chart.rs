//! A live [`BarChart`] and a way out: four channels sweeping under a cursor
//! that is not on any of them.

use knurl::{
    Area, BarChart, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Msg, Outcome, RenderTarget, Screen, ScreenState, VStack,
};

use crate::{AppEvent, Panel};

const LABELS: [&str; 4] = ["Cpu", "Mem", "Net", "Dsk"];

fn triangle(phase: u32, offset: u32) -> u16 {
    let v = ((phase + offset) / 2) % 200;
    (if v < 100 { v } else { 200 - v }) as u16
}

pub struct ChartScreen {
    state: ScreenState,
    label_w: u16,
    phase: u32,
    values: [u16; 4],
    back: Button<'static>,
}

impl ChartScreen {
    pub fn new(panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            label_w: panel.bar_label_w,
            phase: 0,
            values: [0; 4],
            back: Button::new("< Back"),
        }
    }
}

impl Screen for ChartScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    /// A chart is a readout: the only stop on this screen is the way out.
    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, back, .. } = self;
        f(state.chain(), &mut [back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn tick(&mut self) -> bool {
        self.phase = self.phase.wrapping_add(1);
        for (i, v) in self.values.iter_mut().enumerate() {
            *v = triangle(self.phase, i as u32 * 23);
        }
        true
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let data = [
            (LABELS[0], self.values[0]),
            (LABELS[1], self.values[1]),
            (LABELS[2], self.values[2]),
            (LABELS[3], self.values[3]),
        ];
        BarChart::new(&data)
            .with_label_width(self.label_w)
            .with_max(100)
            .view(target, body);
        self.back.view(target, foot);
    }
}
