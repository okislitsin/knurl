//! **Two forms in a layout** - the composition the catalogue never showed.
//!
//! The screen splits its area and paints a form in each half; the cursor walks
//! from the last field of one into the first field of the other and back. None
//! of that is written here: the forms are two zones in a list, and the chain
//! moves between them when one reports it has run out.
//!
//! Which is also the answer to "should `VStack` hold its children?" - it does
//! not have to. Layout is `split`, focus order is the zone list, and the two are
//! independent on purpose: this screen lays out **sideways** on a wide panel and
//! **downwards** on a narrow one, and the focus order does not change with it.

use knurl::{
    Area, BorderStyle, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Form, FormField, HStack, Msg, Outcome, Padding, RenderTarget, Screen,
    ScreenState, Slider, Toggle, VStack,
};

use crate::{AppEvent, Panel};

/// Below this width the two forms stack instead of sitting side by side.
const WIDE_PX: u16 = 200;

pub struct TwoFormsScreen {
    state: ScreenState,
    left: Form,
    fan: Toggle<'static>,
    speed: Slider<'static>,
    right: Form,
    lamp: Toggle<'static>,
    level: Slider<'static>,
    back: Button<'static>,
}

impl TwoFormsScreen {
    pub fn new(panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            left: Form::new(),
            fan: Toggle::new("Fan").with_on(true),
            speed: Slider::new("Spd")
                .with_range(0, 100)
                .with_step(10)
                .with_value(30)
                .with_label_width(panel.label_w),
            right: Form::new(),
            lamp: Toggle::new("Lamp"),
            level: Slider::new("Lvl")
                .with_range(0, 100)
                .with_step(10)
                .with_value(70)
                .with_label_width(panel.label_w),
            back: Button::new("< Back"),
        }
    }

    /// Both field sets and both forms, named once.
    #[allow(clippy::type_complexity)]
    fn parts(
        &mut self,
    ) -> (
        &mut ScreenState,
        (&mut Form, [&mut dyn FormField; 2]),
        (&mut Form, [&mut dyn FormField; 2]),
        &mut Button<'static>,
    ) {
        let Self {
            state,
            left,
            fan,
            speed,
            right,
            lamp,
            level,
            back,
        } = self;
        (state, (left, [fan, speed]), (right, [lamp, level]), back)
    }
}

impl Screen for TwoFormsScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    /// Three zones: form, form, way out. The order here *is* the focus order.
    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let (state, (left, mut lf), (right, mut rf), back) = self.parts();
        let mut a = left.zone(&mut lf);
        let mut b = right.zone(&mut rf);
        f(state.chain(), &mut [&mut a, &mut b, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        let [first, second] = if body.w >= WIDE_PX {
            HStack::split(body, &[Fill(1), Fill(1)])
        } else {
            VStack::split(body, &[Fill(1), Fill(1)])
        };

        let (_, (left, lf), (right, rf), back) = self.parts();
        pane(target, first, left, &lf);
        pane(target, second, right, &rf);
        back.view(target, foot);
    }
}

/// One form inside a rounded box, indented off it by a `Padding` - the layout
/// primitives doing what the old "Layout" page only pointed at.
fn pane(target: &mut dyn RenderTarget, area: Area, form: &Form, fields: &[&mut dyn FormField]) {
    target.draw_box(area, BorderStyle::Rounded);
    // Indented sideways only: on a 128x64 panel a uniform inset costs a whole
    // field row, and the box's own hairline already separates the panes.
    if let Some(inner) = Padding::new(1, 3, 1, 3).inner(area) {
        form.view(target, inner, fields);
    }
}
