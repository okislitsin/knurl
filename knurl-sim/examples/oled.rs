//! OLED demo - a **full component catalog** for a small monochrome
//! SSD1306-class panel, pixel-native throughout.
//!
//! - Navigation is driven by [`Router`] (dogfooded): a root menu (`List`) whose
//!   selection `push`es a component page; every page has a focusable `< Back`
//!   item that `pop`s; the root `Exit` quits via [`Frame::Quit`]. **Back is always
//!   a separate item, never inside a widget's data.**
//! - Input is routed by [`FocusChain`]: each page lists its focus zones once
//!   (see `Demo::with_zones`) and the chain walks the cursor between them, so
//!   "`Down` past the end of the list lands on `< Back`" is not page code. The
//!   application only acts on what comes back out - an `Activated` from the
//!   last zone is a `pop`. Tabs are a [`TabPages`]: rotation switches tabs,
//!   `Select` would enter the page under them.
//! - Rendered through the **dirty-gated partial-redraw loop**
//!   ([`Simulator::run_gated`]): idle frames are skipped, and a painted frame
//!   redraws only what changed (an animating `Spinner` repaints its own area, not
//!   the whole screen). Page transitions force a clean full redraw.
//! - **Scroll-always:** any page taller than the ~5 rows of a 128x64 panel
//!   scrolls (lists/tree/table/pager/help/radio scroll themselves; the menu and
//!   the row-stack pages scroll with a `Scrollbar`). Nothing is truncated.
//! - Encoder model only: Up/Down/Select. Editable fields show a visible edit cue.
//!
//! ASCII text only (the mono font is ASCII; non-ASCII renders blank).
//!
//! ```sh
//! cargo run -p knurl-sim --example oled              # 128x64 (default)
//! cargo run -p knurl-sim --example oled -- 128x128   # 128x128
//! ```

use core::cell::Cell;

use knurl_sim::core::{
    Align, Area, BarChart, BorderStyle, Button, Checkbox, Component,
    Constraint::{Fill, Length},
    Counter, Dialog, Entry, FocusChain, FocusZone, Form, FormField, HStack, Help, Label, LineGauge,
    List, Msg, Outcome, Padded, Padding, Pager, Paginator, Picker, ProgressBar, Radio,
    RenderTarget, Router, Scrollbar, Separator, Slider, Spinner, StatusBar, Style, TabPages, Table,
    Tabs, TextInput, Title, Toggle, Tree, TreeItem, VStack,
};
use knurl_sim::{Frame, SimConfig, Simulator};

// ── Catalog data (all ASCII) ───────────────────────────────────────────────────

const MENU: &[&str] = &[
    "Text",
    "List",
    "Tree",
    "Table",
    "Bar chart",
    "Toggles",
    "Editors",
    "Radio",
    "Text input",
    "Pager",
    "Indicators",
    "Position",
    "Tabs",
    "Status bar",
    "Help",
    "Dialog",
    "Form",
    "Layout",
    "Exit",
];
const LIST_ITEMS: &[&str] = &[
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet",
];
const TREE_ITEMS: &[TreeItem] = &[
    TreeItem::new("src", 0),
    TreeItem::new("main", 1),
    TreeItem::new("lib", 1),
    TreeItem::new("docs", 0),
    TreeItem::new("guide", 1),
    TreeItem::new("readme", 0),
];
const TABLE_ROWS: [[&str; 3]; 4] = [
    ["Bolt", "12", "3"],
    ["Nut", "34", "1"],
    ["Washer", "90", "1"],
    ["Screw", "75", "4"],
];
const TABLE_W: [u16; 3] = [54, 24, 18];
const TABLE_HEADERS: &[&str] = &["Name", "Qt", "P"];
const MODES: &[&str] = &["Eco", "Bal", "Turbo"];
const RADIO_OPTS: &[&str] = &["Low", "Mid", "High"];
const TABS_TITLES: &[&str] = &["One", "Two", "Three"];
const TAB_CONTENT: [[&str; 2]; 3] = [["Alpha", "Bravo"], ["Gamma", "Delta"], ["Echo", "Foxtrot"]];
const HELP_ITEMS: &[(&str, &str)] = &[
    ("Turn", "Move / scroll"),
    ("Push", "Select / edit"),
    ("Back", "Menu item"),
    ("Exit", "Leave demo"),
    ("Edit", "Push a value"),
];
const DIALOG_BTNS: &[&str] = &["OK", "Cancel"];
const PAGER_TEXT: &[&str] = &[
    "knurl is a no_std TUI",
    "kit for tiny encoder",
    "displays. The Pager",
    "scrolls text longer",
    "than the screen, one",
    "line at a time, with",
    "a scrollbar at the",
    "right edge. Nothing",
    "is ever truncated.",
    "Push: go to Back.",
];
const POS_ROWS: &[&str] = &[
    "Row 1", "Row 2", "Row 3", "Row 4", "Row 5", "Row 6", "Row 7", "Row 8", "Row 9", "Row 10",
];

/// 0..=100 triangle wave for the live indicators / bar chart.
fn triangle(phase: u32, offset: u32) -> u16 {
    let v = ((phase + offset) / 2) % 200;
    (if v < 100 { v } else { 200 - v }) as u16
}

// ── Scrollable row stack (Text & Indicators pages) ─────────────────────────────

enum Row {
    Text(&'static str, Style),
    TitleC(&'static str),
    TitleR(&'static str),
    Sep,
    Spacer,
    Spin,
    Bar(u16),   // ProgressBar value 0..=100
    Gauge(u16), // LineGauge value 0..=100
}

/// Draws a vertical row stack scrolled by `scroll`, with a `Scrollbar` on
/// overflow. Clears its own `area` first (it is assembled from transient pieces,
/// so a self-clear keeps scrolling free of stale pixels).
fn draw_stack(
    target: &mut dyn RenderTarget,
    area: Area,
    scroll: usize,
    rows: &[Row],
    spinner: &Spinner,
) {
    if area.w == 0 || area.h == 0 {
        return;
    }
    target.clear(area);
    let lh = target.line_height().max(1);
    let visible = (area.h / lh) as usize;
    let overflow = rows.len() > visible && area.w > 4;
    let w = if overflow { area.w - 4 } else { area.w };

    for r in 0..visible {
        let i = scroll + r;
        if i >= rows.len() {
            break;
        }
        let a = Area::new(area.x, area.y + r as u16 * lh, w, lh);
        match &rows[i] {
            Row::Text(s, st) => Label::new(s).with_style(*st).view(target, a),
            Row::TitleC(s) => Title::new(s).with_align(Align::Center).view(target, a),
            Row::TitleR(s) => Title::new(s).with_align(Align::Right).view(target, a),
            Row::Sep => Separator::new().view(target, a),
            Row::Spacer => {}
            Row::Spin => spinner.view(target, a),
            Row::Bar(v) => {
                let mut pb = ProgressBar::new().with_max(100);
                pb.set_value(*v);
                pb.view(target, a);
            }
            Row::Gauge(v) => {
                let mut g = LineGauge::new().with_max(100);
                g.set_value(*v);
                g.view(target, a);
            }
        }
    }

    if overflow {
        let mut sb = Scrollbar::new();
        sb.set(rows.len(), visible, scroll);
        sb.view(target, Area::new(area.x + area.w - 3, area.y, 3, area.h));
    }
}

// ── Pages ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Menu,
    Text,
    ListP,
    TreeP,
    TableP,
    BarChartP,
    Toggles,
    Editors,
    RadioP,
    TextInputP,
    PagerP,
    Indicators,
    Position,
    TabsP,
    StatusBarP,
    HelpP,
    DialogP,
    FormP,
    Layout,
}

/// Maps a root-menu row to the page it opens; the last row (`Exit`) returns `None`.
fn page_for(i: usize) -> Option<Page> {
    use Page::*;
    [
        Text, ListP, TreeP, TableP, BarChartP, Toggles, Editors, RadioP, TextInputP, PagerP,
        Indicators, Position, TabsP, StatusBarP, HelpP, DialogP, FormP, Layout,
    ]
    .get(i)
    .copied()
}

fn title_for(p: Page) -> &'static str {
    use Page::*;
    match p {
        Menu => "knurl OLED",
        Text => "Text & styles",
        ListP => "List",
        TreeP => "Tree",
        TableP => "Table",
        BarChartP => "Bar chart",
        Toggles => "Toggles",
        Editors => "Editors",
        RadioP => "Radio",
        TextInputP => "Text input",
        PagerP => "Pager",
        Indicators => "Indicators",
        Position => "Position",
        TabsP => "Tabs",
        StatusBarP => "Status bar",
        HelpP => "Help",
        DialogP => "Dialog",
        FormP => "Form",
        Layout => "Layout",
    }
}

/// Whether a page animates every tick (so the gate repaints it on `Tick`).
fn animated(p: Page) -> bool {
    matches!(p, Page::BarChartP | Page::Indicators)
}

/// Field count of the pages built on a [`Form`] (`0` = not a form page). The
/// `< Back` button is always the **last** field, which is how an `Activated`
/// coming out of the form is told apart from one raised by a field.
fn form_len(p: Page) -> usize {
    match p {
        Page::Toggles | Page::FormP => 3,
        Page::Editors => 4,
        Page::TextInputP => 2,
        _ => 0,
    }
}

// ── App-side focus zones ───────────────────────────────────────────────────────

/// The row-stack and Position pages scroll a window over rows they draw by
/// hand, so there is no widget to put on the chain - this is the zone for them.
///
/// It is the whole contract: spend an event while the window can still move,
/// hand it back at either end. The chain does the rest, and "`Down` at the
/// bottom lands on `< Back`" needs no code here at all.
struct ScrollZone<'a> {
    offset: &'a mut usize,
    max: usize,
}

impl FocusZone for ScrollZone<'_> {
    fn handle(&mut self, msg: &Msg) -> Outcome {
        match msg {
            Msg::Up if *self.offset > 0 => {
                *self.offset -= 1;
                Outcome::Consumed
            }
            Msg::Down if *self.offset < self.max => {
                *self.offset += 1;
                Outcome::Consumed
            }
            _ => Outcome::Ignored,
        }
    }
}

/// The Tabs page shows static text, so the page behind the strip is a zone the
/// focus can never enter: `Select` on the strip reports `Ignored` and nothing
/// opens - which is [`TabPages`]' contract for a tab with nothing to focus.
struct StaticPage;

impl FocusZone for StaticPage {
    fn handle(&mut self, _msg: &Msg) -> Outcome {
        Outcome::Ignored
    }
    fn is_focusable(&self) -> bool {
        false
    }
}

struct Demo {
    router: Router<Page, 4>,
    chain: FocusChain,
    tab_pages: TabPages,
    title: Title<'static>,
    menu: List<'static>,

    scroll: usize,
    vis: Cell<usize>, // visible stack rows, cached by view() for update()'s clamp

    // Navigable widgets.
    list: List<'static>,
    tree: Tree<'static>,
    table: Table<'static, [[&'static str; 3]; 4]>,
    radio: Radio<'static>,
    pager: Pager<'static>,
    help: Help<'static>,
    dialog: Dialog<'static>,
    tabs: Tabs<'static>,

    // Form widgets + the Back button.
    form: Form,
    back: Button<'static>,
    chk: Checkbox<'static>,
    tog: Toggle<'static>,
    counter: Counter<'static>,
    slider: Slider<'static>,
    picker: Picker<'static>,
    fan: Toggle<'static>,
    level: Counter<'static>,
    input: TextInput<'static, 12>,

    // Position page + animation state.
    pos_offset: usize,
    spinner: Spinner,
    phase: u32,
    chan: [u16; 4],
    gauge_v: u16,

    // Frame control.
    force: bool,   // structural transition → full clear + repaint everything
    repaint: bool, // something changed this frame → paint (else skip)
    quit: bool,
}

impl Demo {
    fn new() -> Self {
        let mut d = Self {
            router: Router::new(Page::Menu),
            chain: FocusChain::new(),
            tab_pages: TabPages::new(),
            title: Title::new(title_for(Page::Menu)).with_align(Align::Center),
            menu: List::new(MENU),
            scroll: 0,
            vis: Cell::new(4),
            list: List::new(LIST_ITEMS),
            tree: Tree::new(TREE_ITEMS),
            table: Table::new(&TABLE_ROWS, &TABLE_W).with_headers(TABLE_HEADERS),
            radio: Radio::new(RADIO_OPTS),
            pager: Pager::new(PAGER_TEXT),
            help: Help::new(HELP_ITEMS).with_key_width(30),
            dialog: Dialog::new("Save?", "Apply?", DIALOG_BTNS),
            tabs: Tabs::new(TABS_TITLES),
            form: Form::new(),
            back: Button::new("< Back"),
            chk: Checkbox::new("Logging"),
            tog: Toggle::new("Wi-Fi").with_on(true),
            counter: Counter::new("Bright")
                .with_range(0, 100)
                .with_step(10)
                .with_value(60),
            slider: Slider::new("Vol")
                .with_range(0, 100)
                .with_step(10)
                .with_value(40),
            picker: Picker::new("Mode", MODES),
            fan: Toggle::new("Fan"),
            level: Counter::new("Lvl").with_range(0, 5).with_value(2),
            input: TextInput::new("Name").with_label_width(36),
            pos_offset: 0,
            spinner: Spinner::new().with_label("live"),
            phase: 0,
            chan: [0; 4],
            gauge_v: 0,
            force: true,
            repaint: true,
            quit: false,
        };
        d.on_enter(Page::Menu);
        d
    }

    fn page(&self) -> Page {
        self.router.current()
    }

    /// The page's focus zones, in reading order, built for the length of one
    /// call - the same array serves routing, focus placement and invalidation,
    /// so a page describes its focus order exactly once.
    ///
    /// **`< Back` is always the last zone** (or, on a form page, the last
    /// field): that is the demo's whole navigation convention, and the only
    /// thing [`Demo::handle`] needs to know about a page.
    fn with_zones<R>(
        &mut self,
        f: impl FnOnce(&mut FocusChain, &mut [&mut dyn FocusZone]) -> R,
    ) -> R {
        let page = self.router.current();
        let vis = self.vis.get().max(1);
        let Self {
            chain,
            tab_pages,
            menu,
            scroll,
            list,
            tree,
            table,
            radio,
            pager,
            help,
            dialog,
            tabs,
            form,
            back,
            chk,
            tog,
            counter,
            slider,
            picker,
            fan,
            level,
            input,
            pos_offset,
            ..
        } = self;

        match page {
            Page::Menu => f(chain, &mut [menu]),

            // Form pages: Back is the form's last field, so the whole form is
            // one zone on the chain.
            Page::Toggles => {
                let mut fields: [&mut dyn FormField; 3] = [chk, tog, back];
                let mut zone = form.zone(&mut fields);
                f(chain, &mut [&mut zone])
            }
            Page::Editors => {
                let mut fields: [&mut dyn FormField; 4] = [counter, slider, picker, back];
                let mut zone = form.zone(&mut fields);
                f(chain, &mut [&mut zone])
            }
            Page::TextInputP => {
                let mut fields: [&mut dyn FormField; 2] = [input, back];
                let mut zone = form.zone(&mut fields);
                f(chain, &mut [&mut zone])
            }
            Page::FormP => {
                let mut fields: [&mut dyn FormField; 3] = [fan, level, back];
                let mut zone = form.zone(&mut fields);
                f(chain, &mut [&mut zone])
            }

            // Widget pages: the widget, then Back.
            Page::ListP => f(chain, &mut [list, back]),
            Page::TreeP => f(chain, &mut [tree, back]),
            Page::TableP => f(chain, &mut [table, back]),
            Page::RadioP => f(chain, &mut [radio, back]),
            Page::PagerP => f(chain, &mut [pager, back]),
            Page::HelpP => f(chain, &mut [help, back]),
            Page::DialogP => f(chain, &mut [dialog, back]),

            // Hand-drawn scrolling pages: the app's own zone, then Back.
            Page::Text => f(
                chain,
                &mut [
                    &mut ScrollZone {
                        offset: scroll,
                        max: 9usize.saturating_sub(vis),
                    },
                    back,
                ],
            ),
            Page::Indicators => f(
                chain,
                &mut [
                    &mut ScrollZone {
                        offset: scroll,
                        max: 6usize.saturating_sub(vis),
                    },
                    back,
                ],
            ),
            Page::Position => f(
                chain,
                &mut [
                    &mut ScrollZone {
                        offset: pos_offset,
                        max: POS_ROWS.len().saturating_sub(vis),
                    },
                    back,
                ],
            ),

            // The tab strip and its (static) page are one zone; Back below it.
            Page::TabsP => {
                let mut page = StaticPage;
                let mut zone = tab_pages.zone(tabs, &mut page);
                f(chain, &mut [&mut zone, back])
            }

            // Static pages: Back is all there is.
            Page::BarChartP | Page::StatusBarP | Page::Layout => f(chain, &mut [back]),
        }
    }

    /// Enters `page`: reset transient state, place the focus on the page's first
    /// zone, mark everything on it dirty, and force a clean full redraw.
    fn on_enter(&mut self, page: Page) {
        self.scroll = 0;
        self.force = true;
        self.repaint = true;
        self.title.set_text(title_for(page));
        match page {
            Page::Indicators => self.spinner.mark_dirty(), // drawn inside the stack
            Page::TextInputP => self.input.reset(),
            Page::TabsP => self.tabs.set_selected(0),
            _ => {}
        }
        if form_len(page) > 0 {
            self.form = Form::new();
        }
        // `focus_zone` rather than `sync_focus`: the zone set changed under the
        // chain (a different page), and only an explicit placement re-enters.
        self.with_zones(|chain, z| {
            chain.focus_zone(0, Entry::Top, z);
            chain.mark_dirty(z);
        });
    }

    fn push(&mut self, page: Page) {
        self.router.push(page);
        self.on_enter(page);
    }

    fn pop(&mut self) {
        self.router.pop();
        self.on_enter(self.page());
    }

    // ── per-tick animation ────────────────────────────────────────────────────

    fn tick(&mut self) {
        self.phase = self.phase.wrapping_add(1);
        self.gauge_v = triangle(self.phase, 0);
        for (i, c) in self.chan.iter_mut().enumerate() {
            *c = triangle(self.phase, i as u32 * 23);
        }
        if self.page() == Page::Indicators {
            let _ = self.spinner.update(&Msg::Tick);
        }
        if animated(self.page()) {
            self.repaint = true;
        }
    }

    // ── input ──────────────────────────────────────────────────────────────────

    fn handle(&mut self, msg: &Msg) {
        if matches!(msg, Msg::Tick) {
            self.tick();
            return;
        }
        // Any key press repaints (cheap: a clean widget's view still self-gates).
        self.repaint = true;

        // One line of routing for every page: the chain walks the focus between
        // the page's zones, and only what it hands back is the app's business.
        let (outcome, zones) = self.with_zones(|chain, z| (chain.update(msg, z), z.len()));
        if outcome != Outcome::Activated {
            // Consumed: the page used it. Ignored: the cursor is at the edge of
            // the screen - on encoder hardware that is simply the end of the
            // road, since leaving a page is the `< Back` item's job.
            return;
        }

        match self.page() {
            // The root menu has no Back: a row opens a page, the last one exits.
            Page::Menu => match page_for(self.menu.selected()) {
                Some(p) => self.push(p),
                None => self.quit = true,
            },
            // Either dialog button closes the modal (which one is the app's to
            // read via `selected_button`; this demo does not care).
            Page::DialogP if !self.back_activated(zones) => self.pop(),
            // Everywhere else the only thing that activates is `< Back`.
            _ if self.back_activated(zones) => self.pop(),
            _ => {}
        }
    }

    /// Whether the `Activated` just seen came from the page's `< Back` item.
    ///
    /// `Outcome` says *that* something was activated, not *what*: the focus
    /// index is what says which. Two levels answer it - the chain for a page's
    /// zones, the form for a form page's fields.
    fn back_activated(&self, zones: usize) -> bool {
        match form_len(self.page()) {
            0 => self.chain.focus_index() + 1 == zones,
            fields => self.form.focus_index() + 1 == fields,
        }
    }

    // ── render ───────────────────────────────────────────────────────────────

    fn view(&self, target: &mut dyn RenderTarget) {
        let w = target.width();
        let h = target.height();
        let lh = target.line_height().max(1);
        if w == 0 || h < lh {
            return;
        }

        // Structural transition: wipe the whole screen so the new page paints
        // cleanly over the old one (Cell-backed widgets were marked dirty on enter).
        if self.force {
            target.clear(Area::new(0, 0, w, h));
        }

        let [head, body] = VStack::split(Area::new(0, 0, w, h), &[Length(lh), Fill(1)]);
        self.title.view(target, head);
        let body = Area::new(1, body.y, body.w.saturating_sub(1), body.h);
        self.view_body(target, body);
    }

    fn view_body(&self, target: &mut dyn RenderTarget, body: Area) {
        let lh = target.line_height().max(1);
        match self.page() {
            Page::Menu => self.menu.view(target, body),

            Page::Toggles => {
                let f: [&dyn FormField; 3] = [&self.chk, &self.tog, &self.back];
                self.form.view(target, body, &f);
            }
            Page::Editors => {
                let f: [&dyn FormField; 4] =
                    [&self.counter, &self.slider, &self.picker, &self.back];
                self.form.view(target, body, &f);
            }
            Page::TextInputP => {
                let f: [&dyn FormField; 2] = [&self.input, &self.back];
                self.form.view(target, body, &f);
            }
            Page::FormP => {
                let f: [&dyn FormField; 3] = [&self.fan, &self.level, &self.back];
                self.form.view(target, body, &f);
            }

            Page::ListP => self.body_with_back(target, body, |s, t, a| s.list.view(t, a)),
            Page::TreeP => self.body_with_back(target, body, |s, t, a| s.tree.view(t, a)),
            Page::TableP => self.body_with_back(target, body, |s, t, a| s.table.view(t, a)),
            Page::RadioP => self.body_with_back(target, body, |s, t, a| s.radio.view(t, a)),
            Page::PagerP => self.body_with_back(target, body, |s, t, a| s.pager.view(t, a)),
            Page::HelpP => self.body_with_back(target, body, |s, t, a| s.help.view(t, a)),
            Page::DialogP => self.body_with_back(target, body, |s, t, a| s.dialog.view(t, a)),

            Page::BarChartP => self.body_with_back(target, body, |s, t, a| {
                let data = [
                    ("Cpu", s.chan[0]),
                    ("Mem", s.chan[1]),
                    ("Net", s.chan[2]),
                    ("Dsk", s.chan[3]),
                ];
                BarChart::new(&data)
                    .with_label_width(24)
                    .with_max(100)
                    .view(t, a);
            }),

            Page::TabsP => self.body_with_back(target, body, |s, t, a| s.view_tabs(t, a)),
            Page::StatusBarP => self.body_with_back(target, body, |_s, t, a| {
                let row = Area::new(a.x, a.y, a.w, t.line_height());
                StatusBar::new()
                    .with_left("L")
                    .with_center("CTR")
                    .with_right("R")
                    .view(t, row);
            }),
            Page::Layout => self.body_with_back(target, body, |s, t, a| s.view_layout(t, a)),

            Page::Position => self.body_with_back(target, body, |s, t, a| s.view_position(t, a)),

            Page::Text => {
                let [stack, back] = VStack::split(body, &[Fill(1), Length(lh)]);
                self.vis.set((stack.h / lh) as usize);
                draw_stack(target, stack, self.scroll, &self.rows_text(), &self.spinner);
                self.draw_back(target, back);
            }
            Page::Indicators => {
                let [stack, back] = VStack::split(body, &[Fill(1), Length(lh)]);
                self.vis.set((stack.h / lh) as usize);
                draw_stack(
                    target,
                    stack,
                    self.scroll,
                    &self.rows_indicators(),
                    &self.spinner,
                );
                self.draw_back(target, back);
            }
        }
    }

    /// Renders a body widget above a reserved `< Back` row.
    fn body_with_back(
        &self,
        target: &mut dyn RenderTarget,
        body: Area,
        draw: impl FnOnce(&Self, &mut dyn RenderTarget, Area),
    ) {
        let lh = target.line_height().max(1);
        let [top, back] = VStack::split(body, &[Fill(1), Length(lh)]);
        draw(self, target, top);
        self.draw_back(target, back);
    }

    /// The `< Back` item is the real [`Button`] the chain is focusing, so it
    /// draws itself in the focus language - no page-side state to mirror.
    fn draw_back(&self, target: &mut dyn RenderTarget, area: Area) {
        self.back.view(target, area);
    }

    fn view_tabs(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        self.tabs
            .view(target, Area::new(area.x, area.y, area.w, lh));
        let tab = self.tabs.selected().min(2);
        for (i, item) in TAB_CONTENT[tab].iter().enumerate() {
            let y = area.y + lh + i as u16 * lh;
            Label::new(item)
                .with_style(Style::Normal)
                .view(target, Area::new(area.x, y, area.w, lh));
        }
    }

    fn view_layout(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [top, mid, bot] = VStack::split(area, &[Length(lh), Fill(1), Length(lh)]);
        Label::new("VStack top")
            .with_style(Style::Accent)
            .view(target, top);
        let [l, r] = HStack::split(mid, &[Fill(1), Fill(1)]);
        target.draw_box(l, BorderStyle::Rounded);
        target.draw_box(r, BorderStyle::Rounded);
        Padded::new(Label::new("L"), Padding::uniform(2)).view(target, l);
        Padded::new(Label::new("R"), Padding::uniform(2)).view(target, r);
        Label::new("HStack/box")
            .with_style(Style::Accent)
            .view(target, bot);
    }

    fn view_position(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        // Reserve the bottom row for the Paginator; rows + Scrollbar fill the rest.
        let [rows_area, pag_row] = VStack::split(area, &[Fill(1), Length(lh)]);
        let visible = ((rows_area.h / lh) as usize).clamp(1, POS_ROWS.len());
        self.vis.set(visible); // so the Down-clamp matches what fits
        for r in 0..visible {
            let idx = self.pos_offset + r;
            if idx >= POS_ROWS.len() {
                break;
            }
            let style = if r == 0 { Style::Focus } else { Style::Muted };
            Label::new(POS_ROWS[idx]).with_style(style).view(
                target,
                Area::new(
                    rows_area.x,
                    rows_area.y + r as u16 * lh,
                    rows_area.w.saturating_sub(4),
                    lh,
                ),
            );
        }
        let mut sb = Scrollbar::new();
        sb.set(POS_ROWS.len(), visible, self.pos_offset);
        sb.view(
            target,
            Area::new(
                rows_area.x + rows_area.w - 3,
                rows_area.y,
                3,
                visible as u16 * lh,
            ),
        );
        let pages = POS_ROWS.len() - visible + 1;
        Paginator::new(pages)
            .with_current(self.pos_offset)
            .view(target, pag_row);
    }

    fn rows_text(&self) -> [Row; 9] {
        [
            Row::Text("Normal", Style::Normal),
            Row::Text("Accent", Style::Accent),
            Row::Text("Muted", Style::Muted),
            Row::Text("Danger", Style::Danger),
            Row::Sep,
            Row::TitleC("Centered"),
            Row::TitleR("Right"),
            Row::Spacer,
            Row::Text("after spacer", Style::Muted),
        ]
    }

    fn rows_indicators(&self) -> [Row; 6] {
        [
            Row::Spin,
            Row::Text("Progress", Style::Muted),
            Row::Bar(self.chan[0]),
            Row::Text("Gauge", Style::Muted),
            Row::Gauge(self.gauge_v),
            Row::Text("live values", Style::Muted),
        ]
    }
}

fn parse_size() -> (u32, u32) {
    for arg in std::env::args().skip(1) {
        if let Some((w, h)) = arg.split_once('x')
            && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
        {
            return (w, h);
        }
    }
    (128, 64)
}

fn main() {
    let (width, height) = parse_size();
    let scale = if height <= 64 { 6 } else { 5 };

    let mut sim = Simulator::new(SimConfig {
        width,
        height,
        scale,
        title: format!("knurl OLED - {width}x{height} (Up/Down, Space)"),
        ..Default::default()
    });

    let mut demo = Demo::new();

    sim.run_gated(move |target, msgs| {
        for msg in msgs {
            demo.handle(msg);
        }
        if demo.quit {
            return Frame::Quit;
        }
        if !core::mem::take(&mut demo.repaint) {
            return Frame::Skipped;
        }
        demo.view(target);
        demo.force = false;
        Frame::Painted
    });
}
