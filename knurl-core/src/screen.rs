//! A screen: the unit an application is actually built from.
//!
//! [`FocusChain`] routes between the widgets of *one* screen, but it owns
//! nothing, so the screen above it had to be assembled by hand in every
//! application: a `Cell<bool>` dirty flag per screen, a hand-written
//! invalidation cascade, and the zone array built once for routing and again
//! for drawing - two lists that must agree, with no compiler to say so.
//!
//! [`Screen`] is that missing half. A screen names its zones **once**, says what
//! the outcomes of those zones mean in its own vocabulary
//! ([`Screen::Event`]), and draws itself. Everything between - routing,
//! invalidation, entry, the repaint gate - comes from the trait.
//!
//! ## One screen per file, one `match` in the application
//!
//! [`Screen`] is object safe: `&mut dyn Screen<Event = AppEvent>` is a type, so
//! the application's dispatcher is a `match` that hands back a reference and
//! nothing else. Every screen is its own module, and there is no place left for
//! per-screen logic to accumulate outside it:
//!
//! ```
//! # use knurl_core::{Area, Msg, Nav, Router, Screen};
//! # #[derive(Clone, Copy, PartialEq)] enum Page { Menu, Settings }
//! # enum AppEvent { Open(Page), GoBack }
//! # struct MenuScreen; struct SettingsScreen;
//! # impl Screen for MenuScreen {
//! #     type Event = AppEvent;
//! #     fn state(&mut self) -> &mut knurl_core::ScreenState { unimplemented!() }
//! #     fn zones(&mut self, _f: &mut dyn FnMut(&mut knurl_core::FocusChain, &mut [&mut dyn knurl_core::FocusZone])) {}
//! #     fn on_outcome(&mut self, _m: &Msg, _o: knurl_core::Outcome) -> Option<AppEvent> { None }
//! #     fn draw(&mut self, _t: &mut dyn knurl_core::RenderTarget, _a: Area) {}
//! # }
//! # impl Screen for SettingsScreen {
//! #     type Event = AppEvent;
//! #     fn state(&mut self) -> &mut knurl_core::ScreenState { unimplemented!() }
//! #     fn zones(&mut self, _f: &mut dyn FnMut(&mut knurl_core::FocusChain, &mut [&mut dyn knurl_core::FocusZone])) {}
//! #     fn on_outcome(&mut self, _m: &Msg, _o: knurl_core::Outcome) -> Option<AppEvent> { None }
//! #     fn draw(&mut self, _t: &mut dyn knurl_core::RenderTarget, _a: Area) {}
//! # }
//! struct App {
//!     router: Router<Page, 4>,
//!     menu: MenuScreen,
//!     settings: SettingsScreen,
//! }
//!
//! impl App {
//!     /// The whole dispatcher: which screen is current, and nothing else.
//!     fn screen(&mut self) -> &mut dyn Screen<Event = AppEvent> {
//!         match self.router.current() {
//!             Page::Menu => &mut self.menu,
//!             Page::Settings => &mut self.settings,
//!         }
//!     }
//!
//!     /// ...and the one place events become navigation.
//!     fn handle(&mut self, msg: &Msg) {
//!         let Some(event) = self.screen().update(msg) else { return };
//!         let nav = match event {
//!             AppEvent::Open(page) => Nav::Push(page),
//!             AppEvent::GoBack => Nav::Pop,
//!         };
//!         self.router.apply(nav);
//!         self.screen().enter(); // the new screen places its focus and repaints
//!     }
//! }
//! ```
//!
//! ## Where `< Back` goes
//!
//! On encoder hardware "Back" is a focusable item, never a key - so every screen
//! that can be left has one, and there are two places to put it. The convention:
//!
//! - **a form screen puts it in the form**, as the last [`FormField`](crate::FormField).
//!   It then scrolls with the fields, which is what a 128x64 panel needs: a
//!   `< Back` pinned outside the form would eat a row from a stack that is
//!   already scrolling;
//! - **every other screen gives it its own zone**, last in the list, drawn on a
//!   reserved row. The content above it scrolls inside its own widget, so the
//!   row costs nothing that was not already spent on chrome.
//!
//! Either way the screen asks the button itself
//! ([`Button::take_pressed`](crate::Button::take_pressed)) rather than deducing
//! it from a position.
//!
//! ## Ticks and animation
//!
//! A [`FocusChain`] hands an event to the **focused** zone only, which is right
//! for input and wrong for animation: a spinner three rows below the cursor
//! would never advance. So animation does not go through the chain at all - the
//! application drives it directly, and [`Screen::tick`] is where a screen does
//! that for whatever it owns.

use crate::{Area, Entry, FocusChain, FocusZone, Msg, Outcome, RenderTarget};

// ── ScreenState ──────────────────────────────────────────────────────────────

/// The state every screen has and none should hand-roll: the [`FocusChain`] its
/// zones hang off, and the flag saying its picture is stale.
///
/// A screen declares it as one field - `state: ScreenState` - and reaches it
/// through the two one-line accessors [`Screen::state`] and [`Screen::zones`].
/// Nothing else about a screen is bookkeeping.
///
/// The flag is a plain `bool`, not a `Cell`: a screen paints through
/// [`Screen::view`], which takes `&mut self`, so there is nothing to work
/// around. (Widgets need the `Cell` because [`Component::view`](crate::Component::view)
/// takes `&self` and a container may hold them behind a shared reference.)
#[derive(Debug)]
pub struct ScreenState {
    chain: FocusChain,
    dirty: bool,
}

impl ScreenState {
    /// A screen that has not been painted yet: no focus placed, everything
    /// stale. [`Screen::enter`] is what makes it ready.
    pub const fn new() -> Self {
        Self {
            chain: FocusChain::new(),
            dirty: true,
        }
    }

    /// The screen's focus chain, for [`Screen::zones`] to hand to the routing.
    pub fn chain(&mut self) -> &mut FocusChain {
        &mut self.chain
    }

    /// Which zone holds the cursor - the same index [`Screen::zones`] listed.
    pub fn focus_index(&self) -> usize {
        self.chain.focus_index()
    }

    /// Whether the screen owes a full repaint (see [`Screen::view`]).
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Demands a full repaint: the panel is showing something that belongs to
    /// another screen, or to a layout this one no longer has.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Reads the flag and clears it - what [`Screen::view`] does once per frame.
    pub fn take_dirty(&mut self) -> bool {
        core::mem::take(&mut self.dirty)
    }
}

impl Default for ScreenState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Screen ───────────────────────────────────────────────────────────────────

/// One screen of an application: its zones, what their outcomes mean, and how
/// it is drawn.
///
/// Three methods are the screen's own; the rest is provided.
///
/// ```
/// use knurl_core::{
///     Area, Button, Component, FocusChain, FocusZone, List, Msg, Outcome, RenderTarget,
///     Screen, ScreenState,
/// };
///
/// enum AppEvent { Chose(usize), GoBack }
///
/// struct PickScreen {
///     state: ScreenState,
///     items: List<'static>,
///     back: Button<'static>,
/// }
///
/// impl Screen for PickScreen {
///     type Event = AppEvent;
///
///     fn state(&mut self) -> &mut ScreenState { &mut self.state }
///
///     // The only place this screen's zones are listed - routing, focus
///     // placement and invalidation all run off this one array.
///     fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
///         let Self { state, items, back } = self;
///         f(state.chain(), &mut [items, back]);
///     }
///
///     fn on_outcome(&mut self, _msg: &Msg, outcome: Outcome) -> Option<AppEvent> {
///         if outcome != Outcome::Activated { return None; }
///         if self.back.take_pressed() { return Some(AppEvent::GoBack); }
///         Some(AppEvent::Chose(self.items.selected()))
///     }
///
///     fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
///         let lh = target.line_height();
///         let rows = Area::new(area.x, area.y, area.w, area.h.saturating_sub(lh));
///         self.items.view(target, rows);
///         self.back.view(target, Area::new(area.x, area.y + rows.h, area.w, lh));
///     }
/// }
///
/// let mut screen = PickScreen {
///     state: ScreenState::new(),
///     items: List::new(&["Alpha", "Beta"]),
///     back: Button::new("< Back"),
/// };
/// screen.enter();
///
/// // Down past the end of the list lands on "< Back" - chain behaviour, not
/// // screen code - and pressing it is the screen's own event.
/// assert!(screen.update(&Msg::Down).is_none());
/// assert!(screen.update(&Msg::Down).is_none());
/// assert!(matches!(screen.update(&Msg::Select), Some(AppEvent::GoBack)));
/// ```
///
/// ## A screen is not a form
///
/// The trait is built around **zones**, and a [`Form`](crate::Form) is one kind
/// of zone. A menu is a screen with a single [`List`](crate::List) on it and no
/// form anywhere; a catalogue page is a `Tree` and a button; a screen with
/// nothing focusable at all (a splash, a readout) lists no zones and works -
/// every event comes straight back out through
/// [`on_outcome`](Screen::on_outcome) as [`Ignored`](Outcome::Ignored).
///
/// ## What the screen must not do
///
/// - **no `Cell<bool>` and no `mark_clean`** - the repaint flag lives in
///   [`ScreenState`];
/// - **no hand-written invalidation cascade** - [`invalidate`](Screen::invalidate)
///   walks the same zone list the routing uses, so a zone cannot be forgotten;
/// - **no second zone (or field) array for drawing** - `draw` takes `&mut self`
///   precisely so it can build the very array
///   [`zones`](Screen::zones) does, from one helper, instead of a shadow copy
///   that has to be kept in step by hand.
pub trait Screen {
    /// What this screen tells the application. Usually one enum for the whole
    /// application, so screens can be dispatched as
    /// `&mut dyn Screen<Event = AppEvent>`.
    type Event;

    /// The screen's [`ScreenState`] field. One line: `&mut self.state`.
    fn state(&mut self) -> &mut ScreenState;

    /// The **only** place the screen's zones are listed, in reading order.
    ///
    /// The array is built for the length of one call - so a
    /// [`FormZone`](crate::FormZone), a [`TabZone`](crate::TabZone) or a
    /// [`ScrollZone`](crate::ScrollZone) borrowing the screen's own fields can
    /// be in it - and the same array serves routing, focus placement and
    /// invalidation.
    ///
    /// It takes `&mut dyn FnMut(..)` rather than a generic closure, and returns
    /// nothing, so that the trait stays object safe.
    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone]));

    /// What just happened, in the screen's own vocabulary: called once per
    /// event, after the zones have had it.
    ///
    /// `outcome` is the chain's verdict - [`Activated`](Outcome::Activated) when
    /// something was chosen, [`Ignored`](Outcome::Ignored) when nobody wanted
    /// the event (the cursor is at an edge, or it was never navigation). *Which*
    /// widget was activated is asked of the widget
    /// ([`Button::take_pressed`](crate::Button::take_pressed),
    /// [`List::selected`](crate::List::selected)), never deduced from an index.
    fn on_outcome(&mut self, msg: &Msg, outcome: Outcome) -> Option<Self::Event>;

    /// Lays the screen out and paints it. Widgets self-gate on their own dirty
    /// flags, so this runs every painted frame and normally draws very little.
    ///
    /// Containers do not lay out: splitting the area
    /// ([`VStack::split`](crate::VStack::split)) and choosing what goes where is
    /// the screen's, which is why it is also the screen that decides whether a
    /// row is reserved for `< Back`.
    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area);

    // ── provided: none of this belongs in a screen ──────────────────────────

    /// Routes one event through the zones and reports what the screen made of
    /// it. This is [`zones`](Screen::zones) + [`FocusChain::update`] +
    /// [`on_outcome`](Screen::on_outcome), and never anything else.
    ///
    /// `Msg::Tick` is not usually sent here - see [`tick`](Screen::tick).
    fn update(&mut self, msg: &Msg) -> Option<Self::Event> {
        let mut outcome = Outcome::Ignored;
        self.zones(&mut |chain, zones| outcome = chain.update(msg, zones));
        self.on_outcome(msg, outcome)
    }

    /// Advances whatever the screen animates, and reports whether the frame is
    /// worth painting. Default: nothing animates, nothing to paint.
    ///
    /// Animation deliberately bypasses the chain, which only ever feeds the
    /// focused zone: an indicator the user is not pointing at still has to move.
    fn tick(&mut self) -> bool {
        false
    }

    /// Paints the screen, clearing first if it owes a full repaint.
    ///
    /// The flag gates the **clear**, not the drawing: a screen whose picture is
    /// intact still calls [`draw`](Screen::draw), because that is where each
    /// widget decides for itself whether it moved. Gating the draw as well would
    /// undo partial redraw - an animating spinner could never repaint without
    /// the whole screen doing so too.
    fn view(&mut self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        if self.state().take_dirty() {
            target.clear(area);
        }
        self.draw(target, area);
    }

    /// Marks the screen and every zone on it for repaint - the cascade, over
    /// the same list [`zones`](Screen::zones) declared.
    ///
    /// This is the structural transition: whatever is on the panel belongs to a
    /// different screen, or to a layout this one no longer has.
    fn invalidate(&mut self) {
        self.state().mark_dirty();
        self.zones(&mut |chain, zones| chain.mark_dirty(zones));
    }

    /// Makes the screen current: the cursor goes to its first zone that will
    /// take it, and everything repaints.
    ///
    /// Called when the screen is opened *and* when it is returned to. A screen
    /// with nothing focusable enters nobody and still repaints.
    ///
    /// Override [`on_enter`](Screen::on_enter), not this: a Rust `impl` cannot
    /// call the default body it replaces, so an override here would silently
    /// drop the focus placement.
    fn enter(&mut self) {
        self.on_enter();
        self.zones(&mut |chain, zones| {
            chain.focus_zone(0, Entry::Top, zones);
            chain.mark_dirty(zones);
        });
        self.state().mark_dirty();
    }

    /// Runs at the start of [`enter`](Screen::enter), before the focus is
    /// placed: where a screen resets what should not survive being left -
    /// clearing a half-typed name, going back to the first tab. Default: nothing
    /// to reset.
    fn on_enter(&mut self) {}
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use crate::{Button, Checkbox, Component, Form, FormField, List, NoZone, Style, Toggle};

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Chose(usize),
        Back,
    }

    // ── A screen of two plain zones ─────────────────────────────────────────

    struct PickScreen {
        state: ScreenState,
        items: List<'static>,
        back: Button<'static>,
    }

    impl PickScreen {
        fn new() -> Self {
            const ITEMS: &[&str] = &["Alpha", "Beta"];
            Self {
                state: ScreenState::new(),
                items: List::new(ITEMS),
                back: Button::new("< Back"),
            }
        }
    }

    impl Screen for PickScreen {
        type Event = Event;

        fn state(&mut self) -> &mut ScreenState {
            &mut self.state
        }

        fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
            let Self { state, items, back } = self;
            f(state.chain(), &mut [items, back]);
        }

        fn on_outcome(&mut self, _msg: &Msg, outcome: Outcome) -> Option<Event> {
            if outcome != Outcome::Activated {
                return None;
            }
            if self.back.take_pressed() {
                return Some(Event::Back);
            }
            Some(Event::Chose(self.items.selected()))
        }

        fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
            let lh = target.line_height();
            let rows = Area::new(area.x, area.y, area.w, area.h.saturating_sub(lh));
            self.items.view(target, rows);
            self.back
                .view(target, Area::new(area.x, area.y + rows.h, area.w, lh));
        }
    }

    // ── A form screen, Back as its last field ───────────────────────────────

    struct SettingsScreen {
        state: ScreenState,
        form: Form,
        wifi: Toggle<'static>,
        log: Checkbox<'static>,
        back: Button<'static>,
    }

    impl SettingsScreen {
        fn new() -> Self {
            Self {
                state: ScreenState::new(),
                form: Form::new(),
                wifi: Toggle::new("Wi-Fi"),
                log: Checkbox::new("Log"),
                back: Button::new("< Back"),
            }
        }

        /// The screen's fields, named once. Both `zones` and `draw` run off
        /// this - there is no second array to keep in step.
        fn parts(&mut self) -> (&mut ScreenState, &mut Form, [&mut dyn FormField; 3]) {
            let Self {
                state,
                form,
                wifi,
                log,
                back,
            } = self;
            (state, form, [wifi, log, back])
        }
    }

    impl Screen for SettingsScreen {
        type Event = Event;

        fn state(&mut self) -> &mut ScreenState {
            &mut self.state
        }

        fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
            let (state, form, mut fields) = self.parts();
            let mut zone = form.zone(&mut fields);
            f(state.chain(), &mut [&mut zone]);
        }

        fn on_outcome(&mut self, _msg: &Msg, outcome: Outcome) -> Option<Event> {
            (outcome == Outcome::Activated && self.back.take_pressed()).then_some(Event::Back)
        }

        fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
            let (_, form, fields) = self.parts();
            form.view(target, area, &fields);
        }
    }

    /// A screen with nothing to focus: a readout with no way out of it.
    struct StaticScreen {
        state: ScreenState,
        caption: NoZone,
    }

    impl Screen for StaticScreen {
        type Event = Event;

        fn state(&mut self) -> &mut ScreenState {
            &mut self.state
        }

        fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
            let Self { state, caption } = self;
            f(state.chain(), &mut [caption]);
        }

        fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<Event> {
            None
        }

        fn draw(&mut self, _target: &mut dyn RenderTarget, _area: Area) {}
    }

    /// A screen that lists no zones at all - the degenerate case that must not
    /// panic or spin.
    struct EmptyScreen {
        state: ScreenState,
        ticks: usize,
    }

    impl Screen for EmptyScreen {
        type Event = Event;

        fn state(&mut self) -> &mut ScreenState {
            &mut self.state
        }

        fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
            f(self.state.chain(), &mut []);
        }

        fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<Event> {
            None
        }

        fn tick(&mut self) -> bool {
            self.ticks += 1;
            true
        }

        fn draw(&mut self, _target: &mut dyn RenderTarget, _area: Area) {}
    }

    // ── Routing ─────────────────────────────────────────────────────────────

    /// The chain walks the cursor between the screen's zones, and only what it
    /// hands back becomes an event.
    #[test]
    fn events_come_out_only_where_the_screen_says_they_do() {
        let mut s = PickScreen::new();
        s.enter();

        assert_eq!(s.update(&Msg::Down), None, "a step inside the list");
        assert_eq!(s.update(&Msg::Select), Some(Event::Chose(1)));

        assert_eq!(s.update(&Msg::Down), None, "off the list, onto the button");
        assert_eq!(s.state().focus_index(), 1);
        assert_eq!(s.update(&Msg::Select), Some(Event::Back));
    }

    /// The identity of the pressed item comes from the widget, not from its
    /// position: the same `Activated` means two different things one zone apart.
    #[test]
    fn the_widget_says_who_was_pressed() {
        let mut s = PickScreen::new();
        s.enter();
        assert_eq!(s.update(&Msg::Select), Some(Event::Chose(0)));
        assert!(
            !s.back.take_pressed(),
            "the button must not answer for a press it never got"
        );
    }

    /// Off either end of the chain the event is simply unspent - on encoder
    /// hardware that is the edge of the screen, and leaving is `< Back`'s job.
    #[test]
    fn an_event_nobody_wanted_produces_no_screen_event() {
        let mut s = PickScreen::new();
        s.enter();
        assert_eq!(s.update(&Msg::Up), None);
        assert_eq!(s.update(&Msg::Char('x')), None);
    }

    /// A form is one zone among others, and its `Activated` reaches the screen
    /// through the same path - which is what lets `Back` be a form field.
    #[test]
    fn a_form_screen_hears_its_last_field() {
        let mut s = SettingsScreen::new();
        s.enter();

        assert_eq!(s.update(&Msg::Select), None, "toggled Wi-Fi, not an event");
        assert!(s.wifi.is_on());
        assert_eq!(s.update(&Msg::Down), None);
        assert_eq!(s.update(&Msg::Down), None);
        assert_eq!(s.update(&Msg::Select), Some(Event::Back));
    }

    // ── Entering, invalidation, repainting ──────────────────────────────────

    /// `enter` places the cursor on the first zone that will take it and marks
    /// everything - screen and zones - for repaint.
    #[test]
    fn entering_places_the_focus_and_dirties_everything() {
        let mut s = PickScreen::new();
        s.items.mark_clean();
        s.back.mark_clean();
        s.state.take_dirty();

        s.enter();
        assert_eq!(s.state().focus_index(), 0);
        assert!(s.state().dirty(), "the screen owes a full repaint");
        assert!(s.items.dirty() && s.back.dirty(), "so does every zone");
    }

    /// The cascade reaches *into* a zone - the fields of a form, not just the
    /// form - which is the whole reason it is not a hand-written list.
    #[test]
    fn invalidation_cascades_into_the_zones() {
        let mut s = SettingsScreen::new();
        s.enter();
        s.wifi.mark_clean();
        s.log.mark_clean();
        s.back.mark_clean();
        s.state.take_dirty();

        s.invalidate();
        assert!(s.state().dirty());
        assert!(s.wifi.dirty() && s.log.dirty() && s.back.dirty());
    }

    /// A screen owing a repaint clears its area first; once painted it stops -
    /// so an idle frame costs nothing and each widget keeps its own gate.
    #[test]
    fn view_clears_once_and_then_leaves_the_panel_alone() {
        let area = Area::new(0, 0, 120, 40);
        let mut s = PickScreen::new();
        s.enter();

        let mut first = RecordingTarget::new(120, 40);
        s.view(&mut first, area);
        assert_eq!(
            first.ops().first(),
            Some(&Op::Clear { area }),
            "the transition wipes the previous screen"
        );
        assert!(
            first
                .ops()
                .iter()
                .any(|op| matches!(op, Op::Text { text, .. } if text == "< Back")),
            "and the screen painted"
        );

        let mut second = RecordingTarget::new(120, 40);
        s.view(&mut second, area);
        assert!(
            second.ops().is_empty(),
            "a settled screen draws nothing at all"
        );
    }

    /// Moving the cursor repaints the rows that changed - and only those. The
    /// screen-level flag is not involved: it is for transitions.
    #[test]
    fn a_settled_screen_repaints_only_what_moved() {
        let area = Area::new(0, 0, 120, 40);
        let mut s = PickScreen::new();
        s.enter();
        let mut warm = RecordingTarget::new(120, 40);
        s.view(&mut warm, area);

        // Down off the list moves the focus band from the list onto the button.
        let _ = s.update(&Msg::Down);
        let _ = s.update(&Msg::Down);
        let mut t = RecordingTarget::new(120, 40);
        s.view(&mut t, area);

        assert!(!s.state().dirty(), "no transition happened");
        assert!(
            !t.ops()
                .iter()
                .any(|op| matches!(op, Op::Clear { area: a } if *a == area)),
            "the whole screen must not be wiped for a cursor move"
        );
        assert!(
            t.ops()
                .iter()
                .any(|op| matches!(op, Op::Band { style, .. } if *style == Style::Focus)),
            "the button drew its focus band"
        );
    }

    // ── Degenerate screens ──────────────────────────────────────────────────

    /// Nothing focusable: entering must not spin looking for a home, and every
    /// event belongs to the application.
    #[test]
    fn a_screen_with_nothing_focusable_is_inert_but_alive() {
        let mut s = StaticScreen {
            state: ScreenState::new(),
            caption: NoZone,
        };
        s.enter();
        for msg in [Msg::Up, Msg::Down, Msg::Select] {
            assert_eq!(s.update(&msg), None);
        }
        assert!(s.state().dirty());
    }

    /// ...and a screen with no zones at all behaves the same way.
    #[test]
    fn a_screen_with_no_zones_at_all_is_legal() {
        let mut s = EmptyScreen {
            state: ScreenState::new(),
            ticks: 0,
        };
        s.enter();
        s.invalidate();
        for msg in [Msg::Up, Msg::Down, Msg::Select, Msg::Tick] {
            assert_eq!(s.update(&msg), None);
        }
        assert!(
            s.tick(),
            "animation still runs - it never touched the chain"
        );
        assert_eq!(s.ticks, 1);

        let mut t = RecordingTarget::new(64, 32);
        s.view(&mut t, Area::new(0, 0, 64, 32));
    }

    // ── Object safety ───────────────────────────────────────────────────────

    /// The point of the whole shape: screens of different types dispatched
    /// through one reference, so the application's `match` returns a screen and
    /// contains no logic. This fails to *compile* if the trait stops being
    /// object safe.
    #[test]
    fn screens_dispatch_through_one_dyn_reference() {
        struct App {
            current: usize,
            pick: PickScreen,
            settings: SettingsScreen,
        }
        impl App {
            /// The application's whole dispatcher, and nothing else in it.
            fn screen(&mut self) -> &mut dyn Screen<Event = Event> {
                match self.current {
                    0 => &mut self.pick,
                    _ => &mut self.settings,
                }
            }
        }

        let mut app = App {
            current: 0,
            pick: PickScreen::new(),
            settings: SettingsScreen::new(),
        };
        app.screen().enter();

        let mut events = alloc::vec::Vec::new();
        for msg in [Msg::Down, Msg::Down, Msg::Select, Msg::Select] {
            if let Some(e) = app.screen().update(&msg) {
                // The one place an event turns into navigation.
                events.push(e);
                app.current = 1;
                app.screen().enter();
            }
        }
        // Back off the first screen; the last Select then toggled the first
        // field of the second one, which is not an event.
        assert_eq!(events, [Event::Back]);
        assert!(app.settings.wifi.is_on(), "the second screen took over");
    }
}
