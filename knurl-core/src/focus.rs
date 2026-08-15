//! Screen-level focus: which widget the encoder is currently driving, and what
//! happens when the cursor runs off its end.
//!
//! [`Component::update`](crate::Component::update) reports an [`Outcome`], so a
//! container can finally tell a step from a step that went nowhere. This module
//! is what does the telling: a [`FocusChain`] holds an ordered set of
//! [`FocusZone`]s and moves the focus between them exactly when a zone hands an
//! event back.
//!
//! It routes input and cascades invalidation. It does **not** lay out or draw -
//! the application still splits the screen and calls each widget's `view`, so
//! nothing here constrains where a zone appears.

use crate::{Component, Form, FormField, Msg, Outcome};

// ── FocusZone ────────────────────────────────────────────────────────────────

/// Which side the focus came in from - and therefore where the cursor should
/// land inside the zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// Entered from above (the user was going down): land on the first element.
    Top,
    /// Entered from below (the user was going up): land on the last element.
    Bottom,
}

/// One stop on a [`FocusChain`] - anything the encoder can be handed over to.
///
/// Every [`Component`] is already a zone through a blanket implementation, so
/// widgets join a chain unchanged. The trait exists for the two things a bare
/// `Component` cannot express: **where the cursor lands when the focus arrives**
/// ([`enter`](FocusZone::enter)) and **when the focus must not leave**
/// ([`traps_focus`](FocusZone::traps_focus)).
///
/// ## Why not the same method names as `Component`
///
/// `handle`/`enter`/`leave`/`invalidate` deliberately avoid
/// `update`/`focus`/`blur`/`mark_dirty`. The blanket implementation covers every
/// widget, so a name shared with `Component` would make `list.update(&msg)`
/// ambiguous (E0034) in any module that imports both traits - which is every
/// module that builds a chain. Distinct names keep both in scope at once.
pub trait FocusZone {
    /// Handles one event, reporting what became of it (see [`Outcome`]).
    ///
    /// For a plain widget this is [`Component::update`].
    fn handle(&mut self, msg: &Msg) -> Outcome;

    /// The focus has arrived from `from`.
    ///
    /// The default does nothing; a plain widget gets [`Component::focus`] with
    /// the side ignored, so its cursor stays wherever it was left - fine for a
    /// zone with one stop, but see the note on [`FocusChain`] about
    /// multi-element widgets.
    fn enter(&mut self, from: Entry) {
        let _ = from;
    }

    /// The focus has left this zone. For a plain widget, [`Component::blur`].
    fn leave(&mut self) {}

    /// While `true`, the zone keeps the focus: the chain will not move on even
    /// when [`handle`](FocusZone::handle) reports [`Ignored`](Outcome::Ignored).
    ///
    /// This is what stops an edit mode leaking. A `Counter` sitting at its
    /// maximum reports `Ignored` for another `Up` - correct for the counter,
    /// wrong for the screen, because the user is *inside* the form editing it.
    /// A trapping zone turns that into [`Consumed`](Outcome::Consumed) and the
    /// focus stays put.
    fn traps_focus(&self) -> bool {
        false
    }

    /// Whether the focus may stop here at all. Default `true`.
    ///
    /// A zone that answers `false` is **stepped over**: the chain neither
    /// [enters](FocusZone::enter) it nor hands it an event. That is what a
    /// caption between two widgets needs - a [`Label`](crate::Label) handles
    /// nothing, so giving it the focus costs the user an encoder click that
    /// visibly does nothing.
    ///
    /// For a plain widget this is [`Component::focusable`], so a widget opts
    /// out once and is skipped in every chain it joins.
    fn is_focusable(&self) -> bool {
        true
    }

    /// Marks the zone for repaint - the invalidation cascade. For a plain
    /// widget, [`Component::mark_dirty`].
    fn invalidate(&self) {}
}

/// Every widget is a zone: a chain of plain components needs no adapters.
impl<T: Component + ?Sized> FocusZone for T {
    fn handle(&mut self, msg: &Msg) -> Outcome {
        self.update(msg)
    }

    fn is_focusable(&self) -> bool {
        self.focusable()
    }

    fn enter(&mut self, _from: Entry) {
        self.focus();
    }

    fn leave(&mut self) {
        self.blur();
    }

    fn invalidate(&self) {
        self.mark_dirty();
    }
}

// ── FocusChain ───────────────────────────────────────────────────────────────

/// The screen's focus manager: an ordered set of [`FocusZone`]s, one of which
/// holds the encoder.
///
/// Modelled on [`Form`]: it owns no zones and carries no lifetime - the slice is
/// passed to each call, so there is no heap and no self-referential struct. A
/// `Form` is itself a zone through [`Form::zone`], which is how a screen holding
/// two forms, or a list above a button, is expressed.
///
/// It routes input and cascades invalidation, and that is all: layout and
/// painting stay with the application.
///
/// ## Routing
///
/// The focused zone gets the event first. If it reports anything other than
/// [`Ignored`](Outcome::Ignored), that verdict is the chain's verdict -
/// [`Activated`](Outcome::Activated) included, so the application still hears
/// about a button press. Only an `Ignored` from a zone that does not
/// [trap the focus](FocusZone::traps_focus) moves the focus: `Down` to the next
/// zone (entered [`Top`](Entry::Top)), `Up` to the previous one (entered
/// [`Bottom`](Entry::Bottom)), and the event that moved it counts as spent -
/// exactly how `Form` moves between fields.
///
/// At either end of the chain the event comes back out as `Ignored`. That is
/// the signal to the application, and the hook a chain of chains will hang off
/// later. What the application does with each of the three outcomes - and where
/// [`Router`](crate::Router) comes in - is written out once, in the
/// [router docs](crate::Router#where-the-router-meets-the-focus-chain): in
/// short, leaving a screen on encoder hardware is an `Activated` from a "< Back"
/// item, not an `Ignored` off the end.
///
/// Zones that [refuse the focus](FocusZone::is_focusable) - captions, rules,
/// anything static - are **stepped over** on the way past and never entered, so
/// a screen can put its labels in the chain and keep the array in reading
/// order. A chain where *nothing* is focusable is inert: it enters nobody and
/// hands every event back.
///
/// ## Known limitation: where the cursor lands
///
/// For a plain widget [`FocusZone::enter`] is [`Component::focus`], which knows
/// nothing about the side the focus came from, so a multi-element widget keeps
/// the cursor where it was. Walk `Down` out of a `List` left on its last item, come back `Up`,
/// and the cursor is still on that item - which reads fine here, but a widget
/// that should land on a particular element needs its own `enter`. [`Form::zone`]
/// already does this (top → first field, bottom → last field); everything else
/// is per-widget, as and when it is needed.
///
/// ## Warning: a zone with no edges traps the focus forever
///
/// The chain moves on when a zone reports `Ignored`, so a zone that never does
/// can never be left. Two widgets are like this by construction:
/// [`Picker`](crate::Picker) defaults to `wrap == true` (its ends join up), and
/// the [`TextInput`](crate::TextInput) token ribbon is a ring. Inside a `Form`
/// both are safe - `Up`/`Down` only reach them in edit mode, and the way out is
/// `Select` - but as a **bare zone in a chain** either one is a dead end. Use
/// [`Picker::with_wrap(false)`](crate::Picker::with_wrap), or put the widget in
/// a form.
#[derive(Debug, Default)]
pub struct FocusChain {
    focus: usize,
    /// Which zone the chain last handed the focus to, and how many zones there
    /// were at the time - what makes [`sync_focus`](FocusChain::sync_focus)
    /// idempotent (see its docs).
    entered: Option<(usize, usize)>,
}

impl FocusChain {
    pub const fn new() -> Self {
        Self {
            focus: 0,
            entered: None,
        }
    }

    /// Index of the zone currently holding the focus.
    pub fn focus_index(&self) -> usize {
        self.focus
    }

    /// Enters the focused zone and leaves the rest. Call it after building the
    /// screen (and whenever the zone set changes) so the initial focus renders.
    ///
    /// The focused zone is entered [`Top`](Entry::Top) - this is screen setup,
    /// not navigation, so a zone that places its cursor on entry (a form) starts
    /// at its first element. Zones that
    /// [refuse the focus](FocusZone::is_focusable) are skipped, so the cursor
    /// starts on the first zone that can actually use it; if no zone can, every
    /// zone is left and the chain stays out of the way.
    ///
    /// **Calling it after every event is safe.** The chain remembers which zone
    /// it handed the focus to, and re-entering it would be exactly the trap
    /// worth warning about - `enter(Top)` sends a form back to its first field,
    /// so a screen that re-synced on each event would find its cursor unable to
    /// move. A repeat call therefore only re-asserts the other zones' `leave`.
    /// A change in the number of zones counts as a new screen and does enter
    /// again; a set that changed *without* changing length does not, so place
    /// the cursor with [`focus_zone`](FocusChain::focus_zone) after that kind of
    /// change.
    pub fn sync_focus(&mut self, zones: &mut [&mut dyn FocusZone]) {
        let n = zones.len();
        if n == 0 {
            self.entered = None;
            return;
        }
        if self.focus >= n {
            self.focus = n - 1;
        }
        let Some(target) = Self::seek_any(zones, self.focus) else {
            // A chain of captions: nobody here can hold the focus.
            for z in zones.iter_mut() {
                z.leave();
            }
            self.entered = None;
            return;
        };
        self.focus = target;
        let first_time = self.entered != Some((target, n));
        for (i, z) in zones.iter_mut().enumerate() {
            if i == target {
                if first_time {
                    z.enter(Entry::Top);
                }
            } else {
                z.leave();
            }
        }
        self.entered = Some((target, n));
    }

    /// Places the focus on zone `idx` (or the nearest one after it that will
    /// take it), entering it from `from`.
    ///
    /// This is the way to move the focus from **outside** - what a screen does
    /// after rebuilding its zones, or when a menu choice should land the cursor
    /// somewhere specific. Unlike [`sync_focus`](FocusChain::sync_focus) it
    /// always enters, so the target zone places its own cursor afresh.
    pub fn focus_zone(&mut self, idx: usize, from: Entry, zones: &mut [&mut dyn FocusZone]) {
        let n = zones.len();
        if n == 0 {
            return;
        }
        let Some(target) = Self::seek_any(zones, idx.min(n - 1)) else {
            return;
        };
        self.focus = target;
        self.entered = None; // force the enter below
        for (i, z) in zones.iter_mut().enumerate() {
            if i == target {
                z.enter(from);
            } else {
                z.leave();
            }
        }
        self.entered = Some((target, n));
    }

    /// Routes one event, moving the focus between zones when the focused one
    /// runs out of room. See the type docs for the full contract.
    pub fn update(&mut self, msg: &Msg, zones: &mut [&mut dyn FocusZone]) -> Outcome {
        let n = zones.len();
        if n == 0 {
            return Outcome::Ignored;
        }
        if self.focus >= n {
            self.focus = n - 1;
        }

        // 0. The focus can be parked on a zone that will not take it - the
        //    screen never synced, or the zone set changed under it. Move it
        //    before anything is routed; if nothing here is focusable, the whole
        //    chain is inert and every event belongs to the application.
        if !zones[self.focus].is_focusable() {
            match Self::seek_any(zones, self.focus) {
                Some(i) => self.place(zones, i, Entry::Top),
                None => return Outcome::Ignored,
            }
        }

        // 1. The focused zone gets first refusal.
        let outcome = zones[self.focus].handle(msg);
        if outcome != Outcome::Ignored {
            return outcome;
        }

        // 2. It refused - but a zone that holds the focus (a form mid-edit)
        //    keeps it anyway, and the event dies here rather than moving the
        //    cursor out from under the user.
        if zones[self.focus].traps_focus() {
            return Outcome::Consumed;
        }

        // 3. The cursor ran off an end of the zone: hand it to the next
        //    neighbour that can hold it, stepping over the ones that cannot.
        match msg {
            Msg::Down => match Self::seek(zones, self.focus + 1) {
                Some(i) => {
                    self.place(zones, i, Entry::Top);
                    Outcome::Consumed
                }
                None => Outcome::Ignored,
            },
            Msg::Up if self.focus > 0 => match Self::seek_back(zones, self.focus - 1) {
                Some(i) => {
                    self.place(zones, i, Entry::Bottom);
                    Outcome::Consumed
                }
                None => Outcome::Ignored,
            },
            // Either end of the chain, or an event nobody wanted: back out to
            // the application.
            _ => Outcome::Ignored,
        }
    }

    /// Hands the focus to zone `idx`, leaving the one that had it.
    fn place(&mut self, zones: &mut [&mut dyn FocusZone], idx: usize, from: Entry) {
        if idx != self.focus {
            zones[self.focus].leave();
        }
        self.focus = idx;
        zones[idx].enter(from);
        self.entered = Some((idx, zones.len()));
    }

    /// First zone at or after `start` that will take the focus.
    fn seek(zones: &[&mut dyn FocusZone], start: usize) -> Option<usize> {
        (start..zones.len()).find(|&i| zones[i].is_focusable())
    }

    /// Last zone at or before `start` that will take the focus.
    fn seek_back(zones: &[&mut dyn FocusZone], start: usize) -> Option<usize> {
        (0..=start).rev().find(|&i| zones[i].is_focusable())
    }

    /// The nearest zone that will take the focus, looking down from `start`
    /// first and then back up - for placing a cursor rather than moving it,
    /// where either direction is better than nowhere.
    fn seek_any(zones: &[&mut dyn FocusZone], start: usize) -> Option<usize> {
        Self::seek(zones, start).or_else(|| Self::seek_back(zones, start))
    }

    /// Marks every zone dirty - the cascade for a **structural** transition
    /// (navigation, a changed zone set, the first show), where whatever is on
    /// the panel belongs to a different screen.
    ///
    /// Takes the same slice as [`update`](FocusChain::update), so a screen
    /// builds its zone array once and uses it for both.
    pub fn mark_dirty(&self, zones: &[&mut dyn FocusZone]) {
        for z in zones {
            z.invalidate();
        }
    }
}

// ── Form as a zone ───────────────────────────────────────────────────────────

/// A [`Form`] paired with the field slice it operates on, as one [`FocusZone`].
///
/// `Form` takes its fields per call and so is not a [`Component`]; this holds
/// the two together for as long as the chain needs them. Build it fresh per
/// event batch with [`Form::zone`] - it borrows both, exactly as `Form::update`
/// does.
pub struct FormZone<'a, 'f> {
    form: &'a mut Form,
    fields: &'a mut [&'f mut dyn FormField],
}

impl Form {
    /// Pairs this form with its fields as a [`FocusZone`], so a screen can put a
    /// whole form on a [`FocusChain`] beside other widgets.
    ///
    /// ```
    /// use knurl_core::{Button, Checkbox, FocusChain, FocusZone, Form, FormField, List, Msg};
    ///
    /// let mut agree = Checkbox::new("Agree");
    /// let mut back = Button::new("< Back");
    /// let mut list = List::new(&["Alpha", "Beta"]);
    ///
    /// let mut form = Form::new();
    /// let mut chain = FocusChain::new();
    ///
    /// // Built per event batch - the borrows live no longer than the routing.
    /// let mut fields: [&mut dyn FormField; 2] = [&mut agree, &mut back];
    /// let mut form_zone = form.zone(&mut fields);
    /// let mut zones: [&mut dyn FocusZone; 2] = [&mut list, &mut form_zone];
    ///
    /// chain.sync_focus(&mut zones);
    /// let _ = chain.update(&Msg::Down, &mut zones);
    /// ```
    pub fn zone<'a, 'f>(&'a mut self, fields: &'a mut [&'f mut dyn FormField]) -> FormZone<'a, 'f> {
        FormZone { form: self, fields }
    }
}

impl FocusZone for FormZone<'_, '_> {
    fn handle(&mut self, msg: &Msg) -> Outcome {
        self.form.update(msg, self.fields)
    }

    /// Lands on the first field coming down, the last coming up - so a form
    /// entered from below is not skipped over by the very next `Up`.
    fn enter(&mut self, from: Entry) {
        let idx = match from {
            Entry::Top => 0,
            Entry::Bottom => self.fields.len().saturating_sub(1),
        };
        self.form.focus_field(idx, self.fields);
    }

    fn leave(&mut self) {
        // Edit mode must never linger on a zone the focus has left. (The chain
        // will not leave a form mid-edit - that is what traps_focus is for -
        // but a hand-rolled caller might.) It goes through the form rather than
        // through the fields, or `traps_focus()` would keep reporting an edit
        // that no field is in.
        self.form.cancel_edit(self.fields);
        for f in self.fields.iter_mut() {
            f.blur();
        }
    }

    /// While the form is editing a field, the focus belongs to it: a `Counter`
    /// at its bound reports `Ignored`, and without this the chain would walk
    /// away mid-edit.
    fn traps_focus(&self) -> bool {
        self.form.is_editing()
    }

    /// A form whose field set came out empty - every field hidden by the mode
    /// the screen is in - has nowhere to put a cursor, so the focus skips it
    /// instead of stopping on nothing for one click.
    fn is_focusable(&self) -> bool {
        !self.fields.is_empty()
    }

    fn invalidate(&self) {
        for f in self.fields.iter() {
            f.mark_dirty();
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, Checkbox, Counter, Label, List};
    use core::cell::Cell;

    /// A zone that records what the chain did to it. It never handles anything,
    /// so every event walks straight through to the routing rules.
    #[derive(Default)]
    struct Probe {
        focused: Cell<bool>,
        entered_from: Cell<Option<Entry>>,
        invalidated: Cell<usize>,
    }
    impl FocusZone for Probe {
        fn handle(&mut self, _msg: &Msg) -> Outcome {
            Outcome::Ignored
        }
        fn enter(&mut self, from: Entry) {
            self.entered_from.set(Some(from));
            self.focused.set(true);
        }
        fn leave(&mut self) {
            self.focused.set(false);
        }
        fn invalidate(&self) {
            self.invalidated.set(self.invalidated.get() + 1);
        }
    }

    /// A zone that cannot take the focus and counts every attempt to give it
    /// one anyway.
    #[derive(Default)]
    struct Static {
        entered: Cell<usize>,
    }
    impl FocusZone for Static {
        fn handle(&mut self, _msg: &Msg) -> Outcome {
            Outcome::Ignored
        }
        fn enter(&mut self, _from: Entry) {
            self.entered.set(self.entered.get() + 1);
        }
        fn is_focusable(&self) -> bool {
            false
        }
    }

    /// Drives one event through a chain of two forms. The zones are rebuilt per
    /// call - that is the intended usage - so the forms can be inspected between
    /// events.
    fn two_forms(
        chain: &mut FocusChain,
        a: &mut Form,
        fa: &mut [&mut dyn FormField],
        b: &mut Form,
        fb: &mut [&mut dyn FormField],
        msg: &Msg,
    ) -> Outcome {
        let mut za = a.zone(fa);
        let mut zb = b.zone(fb);
        let mut zones: [&mut dyn FocusZone; 2] = [&mut za, &mut zb];
        chain.update(msg, &mut zones)
    }

    /// The headline case: two forms on one screen, the focus walking between
    /// them and landing on the right field at each hand-over.
    #[test]
    fn focus_walks_from_one_form_into_the_next_and_back() {
        let (mut a1, mut a2) = (Checkbox::new("a1"), Checkbox::new("a2"));
        let (mut b1, mut b2) = (Checkbox::new("b1"), Checkbox::new("b2"));
        let mut fa: [&mut dyn FormField; 2] = [&mut a1, &mut a2];
        let mut fb: [&mut dyn FormField; 2] = [&mut b1, &mut b2];
        let (mut a, mut b) = (Form::new(), Form::new());
        let mut chain = FocusChain::new();

        // Down inside form A: the form spends that itself.
        assert_eq!(
            two_forms(&mut chain, &mut a, &mut fa, &mut b, &mut fb, &Msg::Down),
            Outcome::Consumed
        );
        assert_eq!(chain.focus_index(), 0);
        assert_eq!(a.focus_index(), 1, "still inside form A");

        // Down off form A's last field: the chain moves on, and form B is
        // entered from the top - on its FIRST field.
        assert_eq!(
            two_forms(&mut chain, &mut a, &mut fa, &mut b, &mut fb, &Msg::Down),
            Outcome::Consumed
        );
        assert_eq!(chain.focus_index(), 1, "the focus is in form B");
        assert_eq!(b.focus_index(), 0, "entered from the top → first field");

        // Up off form B's first field: back into form A, entered from below -
        // on its LAST field. Landing on the first one would let the next Up
        // throw the focus straight out again, skipping the form.
        assert_eq!(
            two_forms(&mut chain, &mut a, &mut fa, &mut b, &mut fb, &Msg::Up),
            Outcome::Consumed
        );
        assert_eq!(chain.focus_index(), 0);
        assert_eq!(a.focus_index(), 1, "entered from below → last field");
    }

    /// Rule 3: a form in edit mode keeps the focus even when the field it is
    /// editing has nothing left to give. Without `traps_focus` the chain walks
    /// away mid-edit and the counter is left highlighted behind it.
    #[test]
    fn an_edit_does_not_leak_out_of_its_form() {
        let mut counter = Counter::new("N").with_range(0, 1).with_value(1);
        let mut next = Button::new("Go");
        let mut fields: [&mut dyn FormField; 1] = [&mut counter];
        let mut form = Form::new();
        let mut chain = FocusChain::new();

        {
            let mut zone = form.zone(&mut fields);
            let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut next];
            // Select enters edit mode on the counter.
            assert_eq!(chain.update(&Msg::Select, &mut zones), Outcome::Consumed);
        }
        assert!(form.is_editing());

        {
            let mut zone = form.zone(&mut fields);
            let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut next];
            // The counter is at its maximum, so it reports Ignored - but the
            // user is inside the edit, so the chain must not hand the focus on.
            assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Consumed);
            // Same for a Down that the counter cannot use either way round.
            assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
        }
        assert_eq!(chain.focus_index(), 0, "the focus stayed in the form");
        assert!(form.is_editing(), "and the edit is still running");
    }

    #[test]
    fn a_press_inside_a_form_zone_reaches_the_application() {
        let mut go = Button::new("Go");
        let mut fields: [&mut dyn FormField; 1] = [&mut go];
        let mut form = Form::new();
        let mut chain = FocusChain::new();

        let outcome = {
            let mut zone = form.zone(&mut fields);
            let mut zones: [&mut dyn FocusZone; 1] = [&mut zone];
            chain.update(&Msg::Select, &mut zones)
        };
        assert_eq!(outcome, Outcome::Activated);
        assert_eq!(
            form.focus_index(),
            0,
            "and the form says which field it was"
        );
    }

    /// What `zone_nav` does by hand in the demos today: run off the end of a
    /// list and the focus lands on the button below it.
    #[test]
    fn a_plain_widget_hands_the_focus_on_at_its_edge() {
        const ITEMS: &[&str] = &["Alpha", "Beta"];
        let mut list = List::new(ITEMS);
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();
        let mut zones: [&mut dyn FocusZone; 2] = [&mut list, &mut back];

        chain.sync_focus(&mut zones);
        assert_eq!(
            chain.update(&Msg::Down, &mut zones),
            Outcome::Consumed,
            "a step inside the list"
        );
        assert_eq!(chain.focus_index(), 0);

        assert_eq!(
            chain.update(&Msg::Down, &mut zones),
            Outcome::Consumed,
            "off the last item → onto the button"
        );
        assert_eq!(chain.focus_index(), 1);

        // The button is now the one being driven.
        assert_eq!(chain.update(&Msg::Select, &mut zones), Outcome::Activated);

        // And Up walks back into the list, which keeps its own cursor.
        assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Consumed);
        assert_eq!(chain.focus_index(), 0);
    }

    #[test]
    fn the_ends_of_the_chain_report_back_to_the_application() {
        const ITEMS: &[&str] = &["Alpha", "Beta"];
        let mut list = List::new(ITEMS);
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();
        let mut zones: [&mut dyn FocusZone; 2] = [&mut list, &mut back];

        // Top of the first zone, first item: nobody above.
        assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Ignored);

        // Walk to the last zone, then off its bottom: nobody below.
        while chain.focus_index() + 1 < 2 {
            let _ = chain.update(&Msg::Down, &mut zones);
        }
        assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Ignored);

        // An event nobody wants comes straight back too.
        assert_eq!(chain.update(&Msg::Char('x'), &mut zones), Outcome::Ignored);
    }

    #[test]
    fn an_empty_chain_hands_everything_back() {
        let mut chain = FocusChain::new();
        let mut none: [&mut dyn FocusZone; 0] = [];
        for msg in [Msg::Up, Msg::Down, Msg::Select] {
            assert_eq!(chain.update(&msg, &mut none), Outcome::Ignored);
        }
    }

    /// `leave()` cleared the fields' edit flags but not the form's, so the zone
    /// went on reporting an edit that no field was in - and a zone that traps
    /// the focus with nothing to show for it is a screen the user cannot leave.
    /// The chain itself never gets here (it will not walk out of a trapping
    /// zone), but a hand-rolled screen calling `leave()` does.
    #[test]
    fn leaving_a_form_zone_by_hand_ends_the_edit_on_both_sides() {
        let mut counter = Counter::new("N").with_range(0, 9).with_value(1);
        let mut fields: [&mut dyn FormField; 1] = [&mut counter];
        let mut form = Form::new();

        {
            let mut zone = form.zone(&mut fields);
            assert_eq!(zone.handle(&Msg::Select), Outcome::Consumed);
            assert!(zone.traps_focus(), "editing - the zone holds the focus");

            zone.leave();
            assert!(!zone.traps_focus(), "the zone still claims an edit");
        }
        assert!(!form.is_editing(), "the form still thinks it is editing");
    }

    #[test]
    fn sync_focus_enters_one_zone_and_leaves_the_rest() {
        let mut first = Probe::default();
        let mut second = Probe::default();
        let mut chain = FocusChain::new();

        {
            let mut zones: [&mut dyn FocusZone; 2] = [&mut first, &mut second];
            chain.sync_focus(&mut zones);
        }
        assert!(first.focused.get());
        assert!(!second.focused.get());

        {
            let mut zones: [&mut dyn FocusZone; 2] = [&mut first, &mut second];
            // Neither probe handles anything, so Down walks the chain.
            assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
        }
        assert!(!first.focused.get(), "the old zone was left");
        assert!(second.focused.get());
        assert_eq!(
            second.entered_from.get(),
            Some(Entry::Top),
            "arrived through enter(), from above"
        );

        {
            let mut zones: [&mut dyn FocusZone; 2] = [&mut first, &mut second];
            assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Consumed);
        }
        assert_eq!(
            first.entered_from.get(),
            Some(Entry::Bottom),
            "re-entered from below"
        );
    }

    // ── sync_focus called too often ─────────────────────────────────────────

    /// The trap `sync_focus` used to be: `enter(Top)` sends a form back to its
    /// first field, so a screen that re-synced after every event (the shape
    /// every hand-rolled screen has) could never move its cursor. Re-syncing is
    /// now a no-op on a chain that already placed its focus.
    #[test]
    fn re_syncing_after_every_event_leaves_the_cursor_alone() {
        let (mut one, mut two) = (Checkbox::new("1"), Checkbox::new("2"));
        let mut fields: [&mut dyn FormField; 2] = [&mut one, &mut two];
        let mut form = Form::new();
        let mut chain = FocusChain::new();

        {
            let mut zone = form.zone(&mut fields);
            let mut zones: [&mut dyn FocusZone; 1] = [&mut zone];
            chain.sync_focus(&mut zones);
            let _ = chain.update(&Msg::Down, &mut zones);
            chain.sync_focus(&mut zones); // the tempting extra call
        }
        assert_eq!(form.focus_index(), 1, "the form jumped back to field 0");
    }

    /// ...but placing the cursor on purpose still enters the zone afresh.
    #[test]
    fn focus_zone_places_the_cursor_from_outside() {
        let (mut one, mut two) = (Checkbox::new("1"), Checkbox::new("2"));
        let mut fields: [&mut dyn FormField; 2] = [&mut one, &mut two];
        let mut form = Form::new();
        let mut chain = FocusChain::new();

        {
            let mut zone = form.zone(&mut fields);
            let mut zones: [&mut dyn FocusZone; 1] = [&mut zone];
            chain.sync_focus(&mut zones);
            let _ = chain.update(&Msg::Down, &mut zones);
            chain.focus_zone(0, Entry::Top, &mut zones);
        }
        assert_eq!(form.focus_index(), 0, "re-entered from the top");
    }

    // ── Zones that cannot take the focus ────────────────────────────────────

    /// The layout case: captions between the widgets. A `Label` handles
    /// nothing, so the chain used to hand it the focus and then hand the next
    /// event straight back - one encoder click that did nothing the user could
    /// see. The chain now steps over it.
    #[test]
    fn a_label_in_the_chain_is_stepped_over() {
        const ITEMS: &[&str] = &["Alpha", "Beta"];
        let mut caption = Label::new("Settings");
        let mut list = List::new(ITEMS);
        let mut spacer = Label::new("Danger zone");
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();
        let mut zones: [&mut dyn FocusZone; 4] = [&mut caption, &mut list, &mut spacer, &mut back];

        // Setup lands on the list, not on the caption above it.
        chain.sync_focus(&mut zones);
        assert_eq!(chain.focus_index(), 1);

        // Down inside the list, then off its end: straight onto the button.
        assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
        assert_eq!(chain.focus_index(), 1, "still in the list");
        assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
        assert_eq!(chain.focus_index(), 3, "the label in between was skipped");
        assert_eq!(chain.update(&Msg::Select, &mut zones), Outcome::Activated);

        // And back up, over the same label, onto the list's last item.
        assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Consumed);
        assert_eq!(chain.focus_index(), 1);
        assert_eq!(
            chain.update(&Msg::Up, &mut zones),
            Outcome::Consumed,
            "a step inside the list"
        );
        // Above the list there is only the caption: the event leaves the chain
        // instead of parking the focus on it.
        assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Ignored);
        assert_eq!(chain.focus_index(), 1);
    }

    /// A static zone is never entered - not on setup, not in passing.
    #[test]
    fn a_zone_that_refuses_the_focus_is_never_entered() {
        let mut head = Static::default();
        let mut go = Button::new("Go");
        let mut tail = Static::default();
        let mut chain = FocusChain::new();

        {
            let mut zones: [&mut dyn FocusZone; 3] = [&mut head, &mut go, &mut tail];
            chain.sync_focus(&mut zones);
            assert_eq!(chain.focus_index(), 1);
            for msg in [Msg::Down, Msg::Down, Msg::Up, Msg::Up] {
                assert_eq!(chain.update(&msg, &mut zones), Outcome::Ignored);
            }
            assert_eq!(chain.focus_index(), 1, "the focus never left the button");
        }
        assert_eq!(head.entered.get(), 0);
        assert_eq!(tail.entered.get(), 0);
    }

    /// The whole static half of the catalog: none of these can use an event,
    /// so none of them is a stop for the cursor.
    #[test]
    fn static_widgets_refuse_the_focus() {
        use crate::{
            BarChart, LineGauge, ProgressBar, Scrollbar, Separator, Spacer, Spinner, StatusBar,
            Title,
        };
        const BARS: &[(&str, u16)] = &[("a", 1)];

        let checks: [(&str, bool); 10] = [
            ("Label", Label::new("x").is_focusable()),
            ("Title", Title::new("x").is_focusable()),
            ("Separator", Separator::new().is_focusable()),
            ("Spacer", Spacer::new().is_focusable()),
            ("StatusBar", StatusBar::new().is_focusable()),
            ("ProgressBar", ProgressBar::new().is_focusable()),
            ("LineGauge", LineGauge::new().is_focusable()),
            ("Scrollbar", Scrollbar::new().is_focusable()),
            ("BarChart", BarChart::new(BARS).is_focusable()),
            ("Spinner", Spinner::new().is_focusable()),
        ];
        for (name, focusable) in checks {
            assert!(!focusable, "{name} still takes the focus");
        }

        // ...while everything that can spend an Up/Down stays a stop.
        const ITEMS: &[&str] = &["a"];
        assert!(List::new(ITEMS).is_focusable());
        assert!(Button::new("Go").is_focusable());
        assert!(Checkbox::new("x").is_focusable());
    }

    /// The layout case, end to end: captions and chrome between the widgets.
    #[test]
    fn a_chain_steps_over_the_chrome_around_its_widgets() {
        use crate::{StatusBar, Title};
        const ITEMS: &[&str] = &["Alpha", "Beta"];

        let mut title = Title::new("Sensors");
        let mut list = List::new(ITEMS);
        let mut status = StatusBar::new();
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();
        let mut zones: [&mut dyn FocusZone; 4] = [&mut title, &mut list, &mut status, &mut back];

        chain.sync_focus(&mut zones);
        assert_eq!(chain.focus_index(), 1, "the title is not a stop");
        let _ = chain.update(&Msg::Down, &mut zones); // inside the list
        assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
        assert_eq!(chain.focus_index(), 3, "the status bar was skipped");
    }

    /// A form with no fields is nowhere to put a cursor - the very "click that
    /// does nothing" this is all about.
    #[test]
    fn a_form_with_no_fields_refuses_the_focus() {
        const ITEMS: &[&str] = &["Alpha"];
        let mut list = List::new(ITEMS);
        let mut form = Form::new();
        let mut none: [&mut dyn FormField; 0] = [];
        let mut chain = FocusChain::new();

        let mut zone = form.zone(&mut none);
        let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut list];
        assert!(!zones[0].is_focusable());

        chain.sync_focus(&mut zones);
        assert_eq!(chain.focus_index(), 1, "the focus went straight past it");
    }

    /// The degenerate chain: nothing in it can hold the focus. It must not spin
    /// looking for a home, and every event belongs to the application.
    #[test]
    fn a_chain_with_nothing_focusable_hands_every_event_back() {
        let mut a = Label::new("one");
        let mut b = Static::default();
        let mut chain = FocusChain::new();
        let mut zones: [&mut dyn FocusZone; 2] = [&mut a, &mut b];

        chain.sync_focus(&mut zones);
        for msg in [Msg::Up, Msg::Down, Msg::Select, Msg::Down] {
            assert_eq!(chain.update(&msg, &mut zones), Outcome::Ignored);
        }
    }

    #[test]
    fn mark_dirty_cascades_over_every_zone_and_into_a_form() {
        let mut probe = Probe::default();
        let mut cb = Checkbox::new("A");
        let mut go = Button::new("Go");
        let mut fields: [&mut dyn FormField; 2] = [&mut cb, &mut go];
        let mut form = Form::new();
        let chain = FocusChain::new();

        {
            let mut zone = form.zone(&mut fields);
            let zones: [&mut dyn FocusZone; 2] = [&mut probe, &mut zone];
            chain.mark_dirty(&zones);
        }
        assert_eq!(probe.invalidated.get(), 1);
        // The cascade reached the fields inside the form zone, not just the
        // form itself - that is the whole point of a cascade.
        assert!(cb.dirty(), "a field inside the form zone was marked");
    }
}
