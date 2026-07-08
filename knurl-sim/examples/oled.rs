//! OLED demo - a **full component catalog** for a small monochrome
//! SSD1306-class panel, pixel-native throughout.
//!
//! - Navigation is driven by [`Router`] (dogfooded): a root menu (`List`) whose
//!   selection `push`es a component page; every page has a focusable `< Back`
//!   item that `pop`s; the root `Exit` quits via [`Frame::Quit`]. **Back is always
//!   a separate item, never inside a widget's data.**
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
    Counter, Dialog, Form, FormField, HStack, Help, Label, LineGauge, List, Msg, Padded, Padding,
    Pager, Paginator, Picker, ProgressBar, Radio, RenderTarget, Router, Scrollbar, Separator,
    Slider, Spinner, StatusBar, Style, Table, Tabs, TextInput, Title, Toggle, Tree, TreeItem,
    VStack,
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

struct Demo {
    router: Router<Page, 4>,
    title: Title<'static>,
    menu: List<'static>,

    // Two-zone Back state for the navigable / static pages.
    on_back: bool,
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
            title: Title::new(title_for(Page::Menu)).with_align(Align::Center),
            menu: List::new(MENU),
            on_back: false,
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
        d.menu.focus();
        d
    }

    fn page(&self) -> Page {
        self.router.current()
    }

    /// Enters `page`: reset transient state, sync any form focus, mark the page's
    /// Cell-backed widgets dirty, and force a clean full redraw.
    fn on_enter(&mut self, page: Page) {
        self.on_back = false;
        self.scroll = 0;
        self.force = true;
        self.repaint = true;
        self.title.set_text(title_for(page));
        match page {
            Page::Menu => self.menu.mark_dirty(),
            Page::ListP => self.list.mark_dirty(),
            Page::Indicators => self.spinner.mark_dirty(),
            Page::DialogP => {
                self.dialog.reset();
                self.dialog.mark_dirty();
            }
            Page::Toggles => {
                self.form = Form::new();
                let mut f: [&mut dyn FormField; 3] = [&mut self.chk, &mut self.tog, &mut self.back];
                self.form.sync_focus(&mut f);
            }
            Page::Editors => {
                self.form = Form::new();
                let mut f: [&mut dyn FormField; 4] = [
                    &mut self.counter,
                    &mut self.slider,
                    &mut self.picker,
                    &mut self.back,
                ];
                self.form.sync_focus(&mut f);
            }
            Page::TextInputP => {
                self.input.reset();
                self.form = Form::new();
                let mut f: [&mut dyn FormField; 2] = [&mut self.input, &mut self.back];
                self.form.sync_focus(&mut f);
            }
            Page::FormP => {
                self.form = Form::new();
                let mut f: [&mut dyn FormField; 3] =
                    [&mut self.fan, &mut self.level, &mut self.back];
                self.form.sync_focus(&mut f);
            }
            Page::TabsP => self.tabs.set_selected(0),
            _ => {}
        }
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
            self.spinner.update(&Msg::Tick);
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

        match self.page() {
            Page::Menu => match msg {
                Msg::Select => match page_for(self.menu.selected()) {
                    Some(p) => self.push(p),
                    None => self.quit = true,
                },
                _ => self.menu.update(msg),
            },

            // Form pages: the Back is a focusable Button field inside the form.
            Page::Toggles => {
                {
                    let mut f: [&mut dyn FormField; 3] =
                        [&mut self.chk, &mut self.tog, &mut self.back];
                    self.form.update(msg, &mut f);
                }
                self.pop_if_back();
            }
            Page::Editors => {
                {
                    let mut f: [&mut dyn FormField; 4] = [
                        &mut self.counter,
                        &mut self.slider,
                        &mut self.picker,
                        &mut self.back,
                    ];
                    self.form.update(msg, &mut f);
                }
                self.pop_if_back();
            }
            Page::TextInputP => {
                {
                    let mut f: [&mut dyn FormField; 2] = [&mut self.input, &mut self.back];
                    self.form.update(msg, &mut f);
                }
                self.pop_if_back();
            }
            Page::FormP => {
                {
                    let mut f: [&mut dyn FormField; 3] =
                        [&mut self.fan, &mut self.level, &mut self.back];
                    self.form.update(msg, &mut f);
                }
                self.pop_if_back();
            }

            // Navigable widget pages: two-zone Back (Down past the end → Back).
            Page::ListP => {
                let before = self.list.selected();
                if self.zone_nav(
                    msg,
                    before,
                    |s| s.list.update(&Msg::Down),
                    |s| s.list.selected(),
                ) {
                    self.list.update(&Msg::Up);
                }
            }
            Page::TreeP => {
                let before = self.tree.selected();
                if self.zone_nav(
                    msg,
                    before,
                    |s| s.tree.update(&Msg::Down),
                    |s| s.tree.selected(),
                ) {
                    self.tree.update(&Msg::Up);
                }
                if !self.on_back && matches!(msg, Msg::Select) {
                    self.tree.update(&Msg::Select); // expand / collapse
                }
            }
            Page::TableP => {
                let before = self.table.selected();
                self.zone_nav(
                    msg,
                    before,
                    |s| s.table.update(&Msg::Down),
                    |s| s.table.selected(),
                );
                if matches!(msg, Msg::Up) && !self.on_back {
                    self.table.update(&Msg::Up);
                }
            }
            Page::RadioP => {
                let before = self.radio.cursor();
                self.zone_nav(
                    msg,
                    before,
                    |s| s.radio.update(&Msg::Down),
                    |s| s.radio.cursor(),
                );
                if !self.on_back {
                    match msg {
                        Msg::Up => self.radio.update(&Msg::Up),
                        Msg::Select => self.radio.update(&Msg::Select),
                        _ => {}
                    }
                }
            }
            Page::PagerP => {
                let before = self.pager.offset();
                self.zone_nav(
                    msg,
                    before,
                    |s| s.pager.update(&Msg::Down),
                    |s| s.pager.offset(),
                );
                if matches!(msg, Msg::Up) && !self.on_back {
                    self.pager.update(&Msg::Up);
                }
            }
            Page::HelpP => {
                let before = self.help.offset();
                self.zone_nav(
                    msg,
                    before,
                    |s| s.help.update(&Msg::Down),
                    |s| s.help.offset(),
                );
                if matches!(msg, Msg::Up) && !self.on_back {
                    self.help.update(&Msg::Up);
                }
            }

            // Scrollable row-stack pages.
            Page::Text => self.stack_nav(msg, 9),
            Page::Indicators => self.stack_nav(msg, 6),

            // Position: scroll a window, then Back.
            Page::Position => match msg {
                Msg::Up => {
                    if self.on_back {
                        self.on_back = false;
                    } else {
                        self.pos_offset = self.pos_offset.saturating_sub(1);
                    }
                }
                Msg::Down => {
                    if !self.on_back {
                        if self.pos_offset + self.vis.get() < POS_ROWS.len() {
                            self.pos_offset += 1;
                        } else {
                            self.on_back = true;
                        }
                    }
                }
                Msg::Select if self.on_back => self.pop(),
                _ => {}
            },

            // Tabs: Select cycles the active tab; Down → Back; Select on Back pops.
            Page::TabsP => match msg {
                Msg::Up => self.on_back = false,
                Msg::Down => self.on_back = true,
                Msg::Select => {
                    if self.on_back {
                        self.pop();
                    } else {
                        self.tabs.next();
                    }
                }
                _ => {}
            },

            // Static pages: the only focusable item is Back.
            Page::BarChartP | Page::StatusBarP | Page::Layout => match msg {
                Msg::Up => self.on_back = false,
                Msg::Down => self.on_back = true,
                Msg::Select if self.on_back => self.pop(),
                _ => {}
            },

            // Dialog: navigate buttons, Down past the last → Back, Select confirms.
            Page::DialogP => {
                let before = self.dialog.selected();
                self.zone_nav(
                    msg,
                    before,
                    |s| s.dialog.update(&Msg::Down),
                    |s| s.dialog.selected(),
                );
                if !self.on_back {
                    match msg {
                        Msg::Up => self.dialog.update(&Msg::Up),
                        Msg::Select => {
                            self.dialog.update(&Msg::Select);
                            if self.dialog.is_confirmed() {
                                self.pop();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Shared "Down past the end → Back, Select on Back → pop" rule. Returns
    /// `true` when the caller should still apply an `Up` to its widget (i.e. an
    /// `Up` while in the body zone). `advance` performs the widget's `Down`;
    /// `pos` reads the widget's position before/after to detect "couldn't move".
    fn zone_nav(
        &mut self,
        msg: &Msg,
        before: usize,
        advance: impl FnOnce(&mut Self),
        pos: impl FnOnce(&Self) -> usize,
    ) -> bool {
        match msg {
            Msg::Up => {
                if self.on_back {
                    self.on_back = false;
                    false
                } else {
                    true // caller applies the Up to its widget
                }
            }
            Msg::Down => {
                if !self.on_back {
                    advance(self);
                    if pos(self) == before {
                        self.on_back = true;
                    }
                }
                false
            }
            Msg::Select if self.on_back => {
                self.pop();
                false
            }
            _ => false,
        }
    }

    fn stack_nav(&mut self, msg: &Msg, total: usize) {
        let vis = self.vis.get().max(1);
        let max = total.saturating_sub(vis);
        match msg {
            Msg::Up => {
                if self.on_back {
                    self.on_back = false;
                } else {
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            Msg::Down => {
                if !self.on_back {
                    if self.scroll < max {
                        self.scroll += 1;
                    } else {
                        self.on_back = true;
                    }
                }
            }
            Msg::Select if self.on_back => self.pop(),
            _ => {}
        }
    }

    fn pop_if_back(&mut self) {
        if self.back.take_pressed() {
            self.pop();
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

    fn draw_back(&self, target: &mut dyn RenderTarget, area: Area) {
        let style = if self.on_back {
            Style::Focus
        } else {
            Style::Muted
        };
        Label::new("< Back").with_style(style).view(target, area);
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
