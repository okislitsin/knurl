//! **Three forms behind tabs** - a [`TabPages`] whose pages are live forms
//! rather than captions.
//!
//! What the user feels: rotation moves along the strip, `Select` drops into the
//! tab's form, rotating off its top climbs back onto the strip, and off its
//! bottom leaves the tab area for `< Back` underneath. All of it is the
//! container's contract; the screen only says which form belongs to the tab
//! that is showing.
//!
//! The one thing the container cannot do for itself is repaint the form that
//! *arrives* on a switch: at that moment it is still holding the one leaving,
//! and the newcomer has been off-screen with a clean dirty flag, so it would
//! draw nothing over its predecessor. [`TabPages::take_switched`] reports the
//! switch and the screen invalidates itself - the line in `on_outcome` below.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    Counter, FocusChain, FocusZone, Form, FormField, Msg, Outcome, Picker, RenderTarget, Screen,
    ScreenState, TabPages, Tabs, Toggle, VStack,
};

use crate::{AppEvent, Panel};

const TITLES: &[&str] = &["Fan", "Net", "Log"];
const RATES: &[&str] = &["1 Hz", "10 Hz", "50 Hz"];
const LEVELS: &[&str] = &["Off", "Warn", "All"];

pub struct TabFormsScreen {
    state: ScreenState,
    pages: TabPages,
    tabs: Tabs<'static>,

    fan_form: Form,
    fan: Toggle<'static>,
    speed: Counter<'static>,

    net_form: Form,
    dhcp: Toggle<'static>,
    rate: Picker<'static>,

    log_form: Form,
    to_uart: Toggle<'static>,
    level: Picker<'static>,

    back: Button<'static>,
}

impl TabFormsScreen {
    pub fn new(_panel: Panel) -> Self {
        Self {
            state: ScreenState::new(),
            pages: TabPages::new(),
            tabs: Tabs::new(TITLES),
            fan_form: Form::new(),
            fan: Toggle::new("On").with_on(true),
            speed: Counter::new("Spd").with_range(0, 5).with_value(2),
            net_form: Form::new(),
            dhcp: Toggle::new("DHCP").with_on(true),
            rate: Picker::new("Rate", RATES),
            log_form: Form::new(),
            to_uart: Toggle::new("UART"),
            level: Picker::new("Lvl", LEVELS),
            back: Button::new("< Back"),
        }
    }

    /// The active tab's form and its fields - the only page the screen ever
    /// hands to the container, and the same one it paints.
    #[allow(clippy::type_complexity)]
    fn parts(
        &mut self,
    ) -> (
        &mut ScreenState,
        &mut TabPages,
        &mut Tabs<'static>,
        &mut Form,
        [&mut dyn FormField; 2],
        &mut Button<'static>,
    ) {
        let Self {
            state,
            pages,
            tabs,
            fan_form,
            fan,
            speed,
            net_form,
            dhcp,
            rate,
            log_form,
            to_uart,
            level,
            back,
        } = self;
        let (form, fields): (&mut Form, [&mut dyn FormField; 2]) = match tabs.selected() {
            0 => (fan_form, [fan, speed]),
            1 => (net_form, [dhcp, rate]),
            _ => (log_form, [to_uart, level]),
        };
        (state, pages, tabs, form, fields, back)
    }
}

impl Screen for TabFormsScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let (state, pages, tabs, form, mut fields, back) = self.parts();
        let mut page = form.zone(&mut fields);
        let mut area = pages.zone(tabs, &mut page);
        f(state.chain(), &mut [&mut area, back]);
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        if self.pages.take_switched() {
            // The form that just arrived has been off-screen and is clean.
            self.invalidate();
        }
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn on_enter(&mut self) {
        self.tabs.set_selected(0);
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [strip, body, foot] = VStack::split(area, &[Length(lh), Fill(1), Length(lh)]);
        let (_, _, tabs, form, fields, back) = self.parts();
        tabs.view(target, strip);
        form.view(target, body, &fields);
        back.view(target, foot);
    }
}
