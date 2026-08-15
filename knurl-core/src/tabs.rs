use crate::{Area, Component, Entry, FocusZone, Msg, Outcome, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

/// Pixel gap between tab titles.
const TAB_GAP: u16 = 8;

// ── Tabs ──────────────────────────────────────────────────────────────────────

/// A single-row tab strip. Each ASCII title is **underlined** so the strip reads
/// as tabs, not bare text: the active tab gets a thick `Accent` underline and
/// `Accent` text; inactive tabs get a thin `Muted` underline and `Muted` text.
#[derive(Debug)]
pub struct Tabs<'a> {
    titles: &'a [&'a str],
    selected: usize,
}

impl<'a> Tabs<'a> {
    pub fn new(titles: &'a [&'a str]) -> Self {
        Self {
            titles,
            selected: 0,
        }
    }

    /// Sets the selected tab, clamped into `[0, len - 1]` (no-op with no tabs).
    pub fn with_selected(mut self, idx: usize) -> Self {
        self.set_selected(idx);
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Title of the active tab, or `""` when there are no tabs.
    pub fn selected_title(&self) -> &'a str {
        self.titles.get(self.selected).copied().unwrap_or("")
    }

    /// Sets the selected tab, clamped into `[0, len - 1]`.
    pub fn set_selected(&mut self, idx: usize) {
        let n = self.titles.len();
        if n > 0 {
            self.selected = idx.min(n - 1);
        }
    }

    /// Advances to the next tab, stopping at the last.
    pub fn next(&mut self) {
        if self.selected + 1 < self.titles.len() {
            self.selected += 1;
        }
    }

    /// Returns to the previous tab, stopping at the first.
    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

impl<'a> Component for Tabs<'a> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        // Switching tab uses the event; the first/last tab is an edge, so the
        // container gets the event back and can move focus off the strip.
        let before = self.selected;
        match msg {
            // Encoder rotation switches tabs; Left/Right kept for keyboards.
            Msg::Down | Msg::Right => self.next(),
            Msg::Up | Msg::Left => self.prev(),
            _ => return Outcome::Ignored,
        }
        if self.selected != before {
            Outcome::Consumed
        } else {
            Outcome::Ignored
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 || self.titles.is_empty() {
            return;
        }
        let cw = target.char_width().max(1);
        let line_h = target.line_height().max(1);

        let mut x = area.x;
        let right = area.x + area.w; // exclusive bound
        for (i, title) in self.titles.iter().enumerate() {
            if x >= right {
                break;
            }
            let max = ((right - x) / cw) as usize;
            let t = truncate(title, max);
            let tw = target.text_width(t);
            let active = i == self.selected;
            let style = if active { Style::Accent } else { Style::Muted };
            target.draw_text(x, area.y, t, style);

            // Underline: 2px Accent for the active tab, 1px Muted otherwise.
            let (uh, ustyle) = if active {
                (2, Style::Accent)
            } else {
                (1, Style::Muted)
            };
            let uy = area.y + line_h.saturating_sub(uh);
            target.fill_rect(Area::new(x, uy, tw, uh), ustyle);

            x = x.saturating_add(tw).saturating_add(TAB_GAP);
        }
    }
}

// ── TabPages ──────────────────────────────────────────────────────────────────

/// The two-mode container behind a tab strip: the encoder is either **on the
/// strip**, rotating through tabs, or **in the page** the active tab shows.
///
/// A [`Tabs`] strip on its own cannot resolve that conflict - one encoder, two
/// things to drive - which is why apps end up hacking it (`Select` cycles the
/// tabs and the page stays dead). `TabPages` is the horizontal twin of
/// [`Form`](crate::Form): it routes one event and reports its [`Outcome`], owns
/// nothing, and **does not draw** - the application still paints the strip and
/// the page wherever it likes.
///
/// The page is one [`FocusZone`] per call, chosen by the application from
/// [`Tabs::selected`] - so there is no collection of pages to own, no heap, and
/// each page can be a widget, a [`FormZone`](crate::FormZone), or a nested
/// [`FocusChain`](crate::FocusChain) wrapper of the application's own making.
///
/// ## What the user feels
///
/// - **On the strip:** rotation moves between tabs. At the first or the last
///   tab the event comes back out [`Ignored`](Outcome::Ignored) - the strip has
///   run out, exactly as a list does, so an enclosing chain can move the focus
///   off the tab area altogether.
/// - **`Select` on the strip** drops into the page, landing on its first stop.
///   A tab whose page cannot take the focus ([`is_focusable`](FocusZone::is_focusable)
///   is `false`) does not open: the press is reported `Ignored` rather than
///   swallowed, since nothing about the screen changed.
/// - **In the page:** everything goes to the page first. Rotating off its
///   **top** climbs back onto the strip (`Consumed` - the user sees the cursor
///   move). Rotating off its **bottom** leaves the tab area entirely: the event
///   comes out `Ignored`, which is what lets the "< Back" button under the tabs
///   get the focus. A page that [traps the focus](FocusZone::traps_focus) - a
///   form mid-edit - keeps it either way.
///
/// Put on a chain with [`zone`](TabPages::zone), the tab area behaves like any
/// other stop: entered from above it starts on the strip, entered from below it
/// starts *inside* the page, so walking back up does not skip the whole thing.
///
/// ## Repainting a switched-to page
///
/// The container only ever holds the **active** tab's page, so it cannot touch
/// the one arriving on a switch: at the moment the strip moves, the page it
/// still has in hand is the one leaving. And a page that has been off-screen is
/// usually clean, so it would draw nothing over the outgoing page's pixels -
/// the screen would show tab 2's title above tab 1's content.
///
/// So the container reports the switch instead:
/// [`take_switched`](TabPages::take_switched) is armed whenever the strip
/// actually changed tab and cleared by the read, and the screen turns that into
/// a repaint of itself - one line, in the same place it reads every other
/// outcome:
///
/// ```ignore
/// fn on_outcome(&mut self, _msg: &Msg, outcome: Outcome) -> Option<Self::Event> {
///     if self.pages.take_switched() {
///         self.invalidate(); // the page that arrived was off-screen and is clean
///     }
///     ...
/// }
/// ```
///
/// ```
/// use knurl_core::{Button, FocusChain, FocusZone, List, Msg, Outcome, TabPages, Tabs};
///
/// let mut tabs = Tabs::new(&["Live", "Setup"]);
/// let mut live = List::new(&["Temp", "Humidity"]);
/// let mut setup = List::new(&["Units", "Rate"]);
/// let mut back = Button::new("< Back");
///
/// let mut pages = TabPages::new();
/// let mut chain = FocusChain::new();
///
/// // Built per event batch: the application picks the active tab's page.
/// let page: &mut dyn FocusZone = match tabs.selected() {
///     0 => &mut live,
///     _ => &mut setup,
/// };
/// let mut area = pages.zone(&mut tabs, page);
/// let mut zones: [&mut dyn FocusZone; 2] = [&mut area, &mut back];
///
/// chain.sync_focus(&mut zones);
/// assert_eq!(chain.update(&Msg::Select, &mut zones), Outcome::Consumed); // into the page
/// ```
#[derive(Debug, Default)]
pub struct TabPages {
    in_content: bool,
    /// Armed when the strip moved to another tab, cleared by
    /// [`take_switched`](TabPages::take_switched).
    switched: bool,
}

impl TabPages {
    pub const fn new() -> Self {
        Self {
            in_content: false,
            switched: false,
        }
    }

    /// Whether the strip has changed tab since the last call, clearing the flag.
    ///
    /// The one thing the container cannot do for the screen: repaint the page
    /// that just arrived (see the type docs). Read it once per event and
    /// invalidate the screen when it says yes.
    pub fn take_switched(&mut self) -> bool {
        core::mem::take(&mut self.switched)
    }

    /// Whether the encoder is currently driving the page rather than the strip.
    ///
    /// The application reads this to render the two states - the strip carries
    /// no focus of its own.
    pub fn in_content(&self) -> bool {
        self.in_content
    }

    /// Puts the encoder back on the strip, leaving the page.
    pub fn focus_strip(&mut self, content: &mut dyn FocusZone) {
        if self.in_content {
            content.leave();
            self.in_content = false;
        }
    }

    /// Routes one event through the tab area and reports its [`Outcome`]. See
    /// the type docs for the whole contract.
    pub fn update(
        &mut self,
        msg: &Msg,
        tabs: &mut Tabs<'_>,
        content: &mut dyn FocusZone,
    ) -> Outcome {
        if !self.in_content {
            return match msg {
                Msg::Select => {
                    // A tab with nothing to focus does not open. Reporting the
                    // press Ignored rather than Consumed keeps the outcome
                    // honest - the screen did not change - and leaves the event
                    // for whoever owns the tab area to spend.
                    if !content.is_focusable() {
                        return Outcome::Ignored;
                    }
                    content.enter(Entry::Top);
                    self.in_content = true;
                    Outcome::Consumed
                }
                // Rotation belongs to the strip; its edges come back Ignored.
                _ => {
                    let before = tabs.selected();
                    let outcome = tabs.update(msg);
                    self.switched |= tabs.selected() != before;
                    outcome
                }
            };
        }

        let outcome = content.handle(msg);
        if outcome != Outcome::Ignored {
            return outcome;
        }
        // The page had nothing to give - but a page holding the focus (a form
        // mid-edit) keeps it, exactly as a chain would treat it.
        if content.traps_focus() {
            return Outcome::Consumed;
        }
        match msg {
            // Off the top of the page: back onto the strip, one visible step.
            Msg::Up => {
                content.leave();
                self.in_content = false;
                Outcome::Consumed
            }
            // Off the bottom: out of the tab area, so a "< Back" under it can
            // take the focus. The page keeps its cursor until something
            // actually leaves the zone.
            _ => Outcome::Ignored,
        }
    }

    /// Pairs the strip and the active tab's page as one [`FocusZone`], so a
    /// whole tab area can sit on a [`FocusChain`](crate::FocusChain) beside
    /// other widgets. Built per event batch, like
    /// [`Form::zone`](crate::Form::zone).
    pub fn zone<'a, 't>(
        &'a mut self,
        tabs: &'a mut Tabs<'t>,
        content: &'a mut dyn FocusZone,
    ) -> TabZone<'a, 't> {
        TabZone {
            pages: self,
            tabs,
            content,
        }
    }
}

/// A [`TabPages`] paired with its strip and the active tab's page, as one
/// [`FocusZone`]. Build it with [`TabPages::zone`].
pub struct TabZone<'a, 't> {
    pages: &'a mut TabPages,
    tabs: &'a mut Tabs<'t>,
    content: &'a mut dyn FocusZone,
}

impl FocusZone for TabZone<'_, '_> {
    fn handle(&mut self, msg: &Msg) -> Outcome {
        self.pages.update(msg, self.tabs, self.content)
    }

    /// From above, the strip; from below, straight into the page - otherwise
    /// walking back up the screen would jump over the page in one step. A page
    /// that cannot hold the focus leaves the cursor on the strip either way.
    fn enter(&mut self, from: Entry) {
        match from {
            Entry::Bottom if self.content.is_focusable() => {
                self.content.enter(Entry::Bottom);
                self.pages.in_content = true;
            }
            _ => self.pages.focus_strip(self.content),
        }
    }

    fn leave(&mut self) {
        // Resetting the mode is the point: without it the tab area comes back
        // believing the focus is inside a page that nothing is pointing at.
        self.pages.focus_strip(self.content);
    }

    /// An edit inside a page belongs to that page: a `Counter` at its bound
    /// reports `Ignored`, and the chain must not read that as "the user left".
    fn traps_focus(&self) -> bool {
        self.pages.in_content && self.content.traps_focus()
    }

    /// A strip with no tabs is nothing to point at, and its page is
    /// unreachable - the focus skips the whole area.
    fn is_focusable(&self) -> bool {
        !self.tabs.titles.is_empty()
    }

    fn invalidate(&self) {
        self.tabs.mark_dirty();
        self.content.invalidate();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use crate::{Button, Counter, FocusChain, Form, FormField, List};
    use alloc::vec::Vec;

    const T: &[&str] = &["One", "Two"];

    // Default RecordingTarget metrics: char_width = 6, line_height = 10.

    fn texts(t: &RecordingTarget) -> Vec<(u16, u16, alloc::string::String, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, style } => Some((*x, *y, text.clone(), *style)),
                _ => None,
            })
            .collect()
    }

    fn fills(t: &RecordingTarget) -> Vec<(Area, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Fill { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tabs_active_and_inactive_styling_with_underline() {
        let tabs = Tabs::new(T);
        let mut t = RecordingTarget::new(120, 12);
        tabs.view(&mut t, Area::new(0, 0, 120, 12));
        let tx = texts(&t);
        // "One" active: Accent text at x=0; "Two" inactive: Muted at x = 18 + 8 = 26.
        assert!(tx.contains(&(0, 0, "One".into(), Style::Accent)));
        assert!(tx.contains(&(26, 0, "Two".into(), Style::Muted)));
        // Active underline: 2px Accent under "One" (width 18, at y = 10 - 2 = 8).
        assert!(fills(&t).contains(&(Area::new(0, 8, 18, 2), Style::Accent)));
        // Inactive underline: 1px Muted under "Two" (at y = 9).
        assert!(fills(&t).contains(&(Area::new(26, 9, 18, 1), Style::Muted)));
    }

    #[test]
    fn tabs_next_prev_and_encoder() {
        let mut tabs = Tabs::new(T);
        let _ = tabs.update(&Msg::Right);
        assert_eq!(tabs.selected(), 1);
        assert_eq!(tabs.selected_title(), "Two");
        let _ = tabs.update(&Msg::Up);
        assert_eq!(tabs.selected(), 0);
    }

    #[test]
    fn tabs_clamp() {
        let mut tabs = Tabs::new(T);
        tabs.prev();
        assert_eq!(tabs.selected(), 0);
        tabs.set_selected(1);
        tabs.next();
        assert_eq!(tabs.selected(), 1);
    }

    #[test]
    fn tabs_active_underline_moves_with_selection() {
        let tabs = Tabs::new(T).with_selected(1);
        let mut t = RecordingTarget::new(120, 12);
        tabs.view(&mut t, Area::new(0, 0, 120, 12));
        // Now "Two" carries the 2px Accent underline.
        assert!(
            fills(&t)
                .iter()
                .any(|(a, st)| a.h == 2 && a.x == 26 && *st == Style::Accent)
        );
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    #[test]
    fn tabs_spend_a_switch_and_hand_back_an_edge() {
        let mut tabs = Tabs::new(T);
        assert_eq!(
            tabs.update(&Msg::Up),
            Outcome::Ignored,
            "already on the first tab"
        );
        assert_eq!(
            tabs.update(&Msg::Down),
            Outcome::Consumed,
            "switched to the next"
        );
        assert_eq!(
            tabs.update(&Msg::Down),
            Outcome::Ignored,
            "already on the last tab"
        );
        assert_eq!(
            tabs.update(&Msg::Select),
            Outcome::Ignored,
            "the strip does not pick"
        );
    }

    // ── TabPages ────────────────────────────────────────────────────────────

    /// A stand-in for a tab's content: two stops, so it has a top and a bottom
    /// edge to run off, and it records whether the focus is inside it.
    #[derive(Default)]
    struct Content {
        at: u8,
        focused: bool,
        entered_from: Option<Entry>,
    }
    impl FocusZone for Content {
        fn handle(&mut self, msg: &Msg) -> Outcome {
            match msg {
                Msg::Down if self.at == 0 => {
                    self.at = 1;
                    Outcome::Consumed
                }
                Msg::Up if self.at == 1 => {
                    self.at = 0;
                    Outcome::Consumed
                }
                Msg::Select => Outcome::Activated,
                _ => Outcome::Ignored,
            }
        }
        fn enter(&mut self, from: Entry) {
            self.focused = true;
            self.entered_from = Some(from);
            self.at = match from {
                Entry::Top => 0,
                Entry::Bottom => 1,
            };
        }
        fn leave(&mut self) {
            self.focused = false;
        }
    }

    /// A tab whose page has nothing to focus (a read-only screen).
    #[derive(Default)]
    struct Inert {
        entered: usize,
    }
    impl FocusZone for Inert {
        fn handle(&mut self, _msg: &Msg) -> Outcome {
            Outcome::Ignored
        }
        fn enter(&mut self, _from: Entry) {
            self.entered += 1;
        }
        fn is_focusable(&self) -> bool {
            false
        }
    }

    /// The headline walk: strip → switch tabs → into the content → back up to
    /// the strip → down out of the whole thing.
    #[test]
    fn the_encoder_walks_strip_content_and_out() {
        let mut tabs = Tabs::new(T);
        let mut content = Content::default();
        let mut pages = TabPages::new();

        // On the strip: rotation switches tabs.
        assert!(!pages.in_content());
        assert_eq!(
            pages.update(&Msg::Down, &mut tabs, &mut content),
            Outcome::Consumed
        );
        assert_eq!(tabs.selected(), 1);

        // Select drops into the page, landing at its top.
        assert_eq!(
            pages.update(&Msg::Select, &mut tabs, &mut content),
            Outcome::Consumed
        );
        assert!(pages.in_content());
        assert_eq!(content.entered_from, Some(Entry::Top));
        assert!(content.focused);

        // Inside: the page spends what it can.
        assert_eq!(
            pages.update(&Msg::Down, &mut tabs, &mut content),
            Outcome::Consumed
        );
        assert_eq!(
            pages.update(&Msg::Down, &mut tabs, &mut content),
            Outcome::Ignored,
            "off the bottom of the page: the screen below gets a turn"
        );
        assert!(pages.in_content(), "and the focus is still in the page");

        // Up off the top of the page climbs back onto the strip.
        assert_eq!(
            pages.update(&Msg::Up, &mut tabs, &mut content),
            Outcome::Consumed,
            "back to the first stop of the page"
        );
        assert_eq!(
            pages.update(&Msg::Up, &mut tabs, &mut content),
            Outcome::Consumed,
            "off the top of the page → the strip"
        );
        assert!(!pages.in_content());
        assert!(!content.focused, "the page was left");

        // And the strip's own edges are handed back out again.
        assert_eq!(
            pages.update(&Msg::Down, &mut tabs, &mut content),
            Outcome::Ignored,
            "already on the last tab"
        );
    }

    /// The switch is reported exactly once, and only when a tab actually
    /// changed - a screen that repainted on every rotation at the last tab
    /// would flicker for nothing.
    #[test]
    fn a_switch_is_reported_once_and_only_when_the_tab_moved() {
        let mut tabs = Tabs::new(T);
        let mut content = Content::default();
        let mut pages = TabPages::new();

        assert!(!pages.take_switched(), "nothing has moved yet");
        let _ = pages.update(&Msg::Down, &mut tabs, &mut content);
        assert!(pages.take_switched(), "tab 0 → 1");
        assert!(!pages.take_switched(), "the read consumed it");

        let _ = pages.update(&Msg::Down, &mut tabs, &mut content); // last tab
        assert!(!pages.take_switched(), "the strip had nowhere to go");

        // Inside the page, rotation belongs to the page, not the strip.
        let _ = pages.update(&Msg::Select, &mut tabs, &mut content);
        let _ = pages.update(&Msg::Down, &mut tabs, &mut content);
        assert!(!pages.take_switched());
    }

    #[test]
    fn both_ends_of_the_strip_hand_the_event_back() {
        let mut tabs = Tabs::new(T);
        let mut content = Content::default();
        let mut pages = TabPages::new();

        assert_eq!(
            pages.update(&Msg::Up, &mut tabs, &mut content),
            Outcome::Ignored,
            "first tab"
        );
        let _ = pages.update(&Msg::Down, &mut tabs, &mut content);
        assert_eq!(
            pages.update(&Msg::Down, &mut tabs, &mut content),
            Outcome::Ignored,
            "last tab"
        );
        assert!(!pages.in_content(), "an edge never moves the focus inwards");
    }

    /// A tab whose page cannot hold the focus: `Select` changes nothing, so it
    /// is reported unspent and whoever owns the screen may use it.
    #[test]
    fn selecting_a_tab_with_nothing_to_focus_hands_the_press_back() {
        let mut tabs = Tabs::new(T);
        let mut inert = Inert::default();
        let mut pages = TabPages::new();

        assert_eq!(
            pages.update(&Msg::Select, &mut tabs, &mut inert),
            Outcome::Ignored
        );
        assert!(!pages.in_content());
        assert_eq!(inert.entered, 0, "an unfocusable page is never entered");
    }

    // ── TabZone: inside a chain ─────────────────────────────────────────────

    /// The screen shape this is all for: a tabbed area with a "< Back" button
    /// underneath it.
    #[test]
    fn a_tab_zone_hands_the_focus_on_to_the_button_below() {
        let mut tabs = Tabs::new(T);
        let mut content = Content::default();
        let mut pages = TabPages::new();
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();

        {
            let mut zone = pages.zone(&mut tabs, &mut content);
            let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut back];
            chain.sync_focus(&mut zones);

            // Into the page, to its last stop, then off the bottom - which the
            // chain spends on moving to the button.
            assert_eq!(chain.update(&Msg::Select, &mut zones), Outcome::Consumed);
            assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
            assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
            assert_eq!(chain.focus_index(), 1, "the focus is on the button");
            assert_eq!(chain.update(&Msg::Select, &mut zones), Outcome::Activated);

            // Up re-enters the tab area from below: straight back into the
            // page, at its last stop - not onto the strip.
            assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Consumed);
            assert_eq!(chain.focus_index(), 0);
        }
        assert!(pages.in_content(), "entered from below → inside the page");
        assert_eq!(content.entered_from, Some(Entry::Bottom));
        assert_eq!(chain.focus_index(), 0, "and the chain is back in the tabs");
    }

    /// Leaving through the chain must reset the mode, or the tab area comes
    /// back believing the focus is in a page nobody is looking at.
    #[test]
    fn leaving_the_zone_puts_the_focus_back_on_the_strip() {
        let mut tabs = Tabs::new(T);
        let mut content = Content::default();
        let mut pages = TabPages::new();
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();

        {
            let mut zone = pages.zone(&mut tabs, &mut content);
            let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut back];
            chain.sync_focus(&mut zones);
            let _ = chain.update(&Msg::Select, &mut zones); // into the page
            let _ = chain.update(&Msg::Down, &mut zones); // to its last stop
            let _ = chain.update(&Msg::Down, &mut zones); // out to the button
        }
        assert!(!pages.in_content(), "the mode was left behind");
        assert!(!content.focused);

        // Coming back from the top (a fresh sync, as after a screen change)
        // lands on the strip.
        {
            let mut zone = pages.zone(&mut tabs, &mut content);
            let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut back];
            chain.focus_zone(0, Entry::Top, &mut zones);
        }
        assert!(!pages.in_content());
    }

    /// A form being edited inside a tab keeps the focus: the counter at its
    /// bound reports `Ignored`, and neither the tab area nor the chain may read
    /// that as "the user left".
    #[test]
    fn an_edit_inside_a_page_does_not_leak_out() {
        let mut tabs = Tabs::new(T);
        let mut counter = Counter::new("N").with_range(0, 1).with_value(1);
        let mut fields: [&mut dyn FormField; 1] = [&mut counter];
        let mut form = Form::new();
        let mut pages = TabPages::new();
        let mut back = Button::new("< Back");
        let mut chain = FocusChain::new();

        let mut form_zone = form.zone(&mut fields);
        {
            let mut zone = pages.zone(&mut tabs, &mut form_zone);
            let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut back];
            chain.sync_focus(&mut zones);
            let _ = chain.update(&Msg::Select, &mut zones); // into the page
            let _ = chain.update(&Msg::Select, &mut zones); // edit the counter
            assert!(zones[0].traps_focus(), "the edit holds the focus");

            // The counter is at its maximum and the form is at its only field,
            // so both Up and Down come back Ignored from inside - and neither
            // may move the focus.
            assert_eq!(chain.update(&Msg::Up, &mut zones), Outcome::Consumed);
            assert_eq!(chain.update(&Msg::Down, &mut zones), Outcome::Consumed);
            assert_eq!(chain.focus_index(), 0);
        }
        assert!(pages.in_content(), "still inside the page");
    }

    /// A strip with no titles is not a place the focus can stop.
    #[test]
    fn a_strip_with_no_tabs_refuses_the_focus() {
        const NONE: &[&str] = &[];
        let mut tabs = Tabs::new(NONE);
        let mut content = Content::default();
        let mut pages = TabPages::new();
        let mut list = List::new(T);
        let mut chain = FocusChain::new();

        let mut zone = pages.zone(&mut tabs, &mut content);
        let mut zones: [&mut dyn FocusZone; 2] = [&mut zone, &mut list];
        assert!(!zones[0].is_focusable());
        chain.sync_focus(&mut zones);
        assert_eq!(chain.focus_index(), 1, "the focus went straight past it");
    }
}
