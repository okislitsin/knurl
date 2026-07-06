//! TFT demo - a **full component catalog** for a 320x240 colour panel, the
//! colour twin of `oled.rs`.
//!
//! - Renders through `ColorSimulator` + the shipped Charm [`ColorTheme`] - calm
//!   palette, no hardcoded RGB, no bright blocks. Selection reads as a lilac
//!   highlight, accents as coloured text.
//! - Navigation is driven by [`Router`] (same pattern as the OLED demo): a root
//!   menu (`List`) whose selection `push`es a component page; every page has a
//!   focusable `< Back` item that `pop`s; the root `Exit` quits via [`Frame::Quit`].
//!   Opens on the menu. **Back is always a separate item, never in widget data.**
//! - Rendered through the **dirty-gated partial-redraw loop**
//!   ([`ColorSimulator::run_gated`]): idle frames are skipped; a painted frame
//!   redraws only what changed (so animation / the streaming Pager repaint their
//!   own area, not the whole 320x240). Transitions force a clean full redraw.
//! - **Realtime Pager page:** a ring-buffer `LinesModel` (`StreamLog`) gains a
//!   line every few ticks - standing in for live UART - with follow/tail mode
//!   keeping the newest line visible.
//! - **Scroll-always:** anything taller than the screen scrolls; nothing truncates.
//! - Encoder model only: Up/Down/Select. Editable fields show an edit cue.
//!
//! ASCII text only (the mono font is ASCII; non-ASCII renders blank).
//!
//! Run with: `cargo run -p knurl-sim --example tft`

use core::cell::Cell;
use std::cell::RefCell;

use knurl_sim::core::{
    Align, Area, BarChart, BorderStyle, Button, Checkbox, Component,
    Constraint::{Fill, Length},
    Counter, Dialog, Form, FormField, HStack, Help, Label, LineGauge, LinesModel, List, Msg, Padded,
    Padding, Pager, Paginator, Picker, ProgressBar, Radio, RenderTarget, Router, Scrollbar,
    Separator, Slider, Spinner, StatusBar, Style, Table, Tabs, TextInput, Title, Toggle, Tree,
    TreeItem, VStack,
};
use knurl_sim::{ColorSimConfig, ColorSimulator, Frame};

// ── Catalog data (all ASCII) ───────────────────────────────────────────────────

const MENU: &[&str] = &[
    "Text", "List", "Tree", "Table", "Bar chart", "Toggles", "Editors", "Radio", "Text input",
    "Pager (live)", "Indicators", "Position", "Tabs", "Status bar", "Help", "Dialog", "Form",
    "Layout", "Exit",
];
const LIST_ITEMS: &[&str] = &[
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet",
    "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
];
const TREE_ITEMS: &[TreeItem] = &[
    TreeItem::new("project", 0),
    TreeItem::new("src", 1),
    TreeItem::new("main", 2),
    TreeItem::new("lib", 2),
    TreeItem::new("docs", 1),
    TreeItem::new("guide", 2),
    TreeItem::new("readme", 1),
];
const TABLE_ROWS: [[&str; 3]; 5] = [
    ["Bolt", "12", "3"],
    ["Nut", "34", "1"],
    ["Washer", "90", "1"],
    ["Screw", "75", "4"],
    ["Rivet", "21", "2"],
];
const TABLE_W: [u16; 3] = [150, 60, 48];
const TABLE_HEADERS: &[&str] = &["Name", "Qty", "Pri"];
const MODES: &[&str] = &["Eco", "Balanced", "Turbo"];
const RADIO_OPTS: &[&str] = &["Low", "Medium", "High"];
const TABS_TITLES: &[&str] = &["Main", "Net", "Info"];
const TAB_CONTENT: [[&str; 2]; 3] = [["Alpha", "Bravo"], ["Gamma", "Delta"], ["Echo", "Foxtrot"]];
const HELP_ITEMS: &[(&str, &str)] = &[
    ("Turn", "Move / scroll the cursor"),
    ("Push", "Select, or edit a value"),
    ("Back", "A focusable menu item"),
    ("Exit", "Leave the demo (root)"),
    ("Edit", "Push to edit, push to commit"),
];
const DIALOG_BTNS: &[&str] = &["OK", "Cancel"];
const POS_ROWS: &[&str] = &[
    "Row 1", "Row 2", "Row 3", "Row 4", "Row 5", "Row 6", "Row 7", "Row 8", "Row 9", "Row 10",
    "Row 11", "Row 12",
];

/// 0..=100 triangle wave for the live indicators / bar chart.
fn triangle(phase: u32, offset: u32) -> u16 {
    let v = ((phase + offset) / 2) % 200;
    (if v < 100 { v } else { 200 - v }) as u16
}

// ── Streaming log (realtime Pager) ─────────────────────────────────────────────

/// A capped ring buffer of recent lines - a stand-in for a live UART log. Uses
/// interior mutability so the app can append (`&self`) while the `Pager` borrows
/// it, and overrides `write_line` so it never hands out a borrow of its buffer.
struct StreamLog {
    lines: RefCell<Vec<String>>,
    cap: usize,
}
impl StreamLog {
    fn new(cap: usize) -> Self {
        Self { lines: RefCell::new(Vec::new()), cap }
    }
    fn push(&self, line: String) {
        let mut v = self.lines.borrow_mut();
        v.push(line);
        if v.len() > self.cap {
            let excess = v.len() - self.cap;
            v.drain(0..excess);
        }
    }
}
impl LinesModel for StreamLog {
    fn line_count(&self) -> usize {
        self.lines.borrow().len()
    }
    fn get_line(&self, _i: usize) -> &str {
        "" // unused: the Pager renders via write_line
    }
    fn write_line(&self, i: usize, out: &mut dyn core::fmt::Write) {
        if let Some(s) = self.lines.borrow().get(i) {
            let _ = out.write_str(s);
        }
    }
}

// ── Scrollable row stack (Text & Indicators pages) ─────────────────────────────

enum Row {
    Text(&'static str, Style),
    TitleC(&'static str),
    TitleR(&'static str),
    Sep,
    Spacer,
    Spin,
    Bar(u16),
    Gauge(u16),
}

/// Draws a vertical row stack scrolled by `scroll`, with a `Scrollbar` on
/// overflow. Clears its own `area` first (it is assembled from transient pieces).
fn draw_stack(target: &mut dyn RenderTarget, area: Area, scroll: usize, rows: &[Row], spinner: &Spinner) {
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
        Menu => "knurl TFT catalog",
        Text => "Text & styles",
        ListP => "List",
        TreeP => "Tree",
        TableP => "Table",
        BarChartP => "Bar chart",
        Toggles => "Toggles",
        Editors => "Value editors",
        RadioP => "Radio",
        TextInputP => "Text input",
        PagerP => "Pager (live stream)",
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

fn hint_for(p: Page) -> &'static str {
    use Page::*;
    match p {
        Menu => "Turn: move   Push: open",
        Toggles | Editors | FormP | TextInputP => "Push: edit / activate Back",
        TreeP => "Push: expand / Back",
        TabsP => "Push: next tab / Back",
        DialogP => "Push: a button / Back",
        PagerP => "Turn: scroll, bottom = follow",
        _ => "Turn: move   Push: Back",
    }
}

fn animated(p: Page) -> bool {
    matches!(p, Page::BarChartP | Page::Indicators)
}

struct Demo {
    router: Router<Page, 4>,
    title: Title<'static>,
    menu: List<'static>,

    on_back: bool,
    scroll: usize,
    vis: Cell<usize>,

    list: List<'static>,
    tree: Tree<'static>,
    table: Table<'static, [[&'static str; 3]; 5]>,
    radio: Radio<'static>,
    help: Help<'static>,
    dialog: Dialog<'static>,
    tabs: Tabs<'static>,

    form: Form,
    back: Button<'static>,
    chk: Checkbox<'static>,
    tog: Toggle<'static>,
    counter: Counter<'static>,
    slider: Slider<'static>,
    picker: Picker<'static>,
    fan: Toggle<'static>,
    level: Counter<'static>,
    input: TextInput<'static, 16>,

    pos_offset: usize,
    spinner: Spinner,
    phase: u32,
    chan: [u16; 4],
    gauge_v: u16,

    force: bool,
    repaint: bool,
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
            vis: Cell::new(8),
            list: List::new(LIST_ITEMS),
            tree: Tree::new(TREE_ITEMS),
            table: Table::new(&TABLE_ROWS, &TABLE_W).with_headers(TABLE_HEADERS),
            radio: Radio::new(RADIO_OPTS),
            help: Help::new(HELP_ITEMS).with_key_width(60),
            dialog: Dialog::new("Save changes?", "Apply the new settings?", DIALOG_BTNS),
            tabs: Tabs::new(TABS_TITLES),
            form: Form::new(),
            back: Button::new("< Back"),
            chk: Checkbox::new("Logging"),
            tog: Toggle::new("Wi-Fi").with_on(true),
            counter: Counter::new("Brightness").with_range(0, 100).with_step(10).with_value(60),
            slider: Slider::new("Volume").with_range(0, 100).with_step(10).with_value(40).with_label_width(72),
            picker: Picker::new("Mode", MODES),
            fan: Toggle::new("Fan"),
            level: Counter::new("Level").with_range(0, 5).with_value(2),
            input: TextInput::new("Name").with_label_width(54),
            pos_offset: 0,
            spinner: Spinner::new().with_label("streaming"),
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
                let mut f: [&mut dyn FormField; 4] =
                    [&mut self.counter, &mut self.slider, &mut self.picker, &mut self.back];
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
                let mut f: [&mut dyn FormField; 3] = [&mut self.fan, &mut self.level, &mut self.back];
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

    /// Handles one message. `pager` is the realtime streaming Pager (a local in
    /// `main`, borrowing the `StreamLog`); only the Pager page touches it.
    fn handle(&mut self, msg: &Msg, pager: &mut Pager<'_, StreamLog>) {
        if matches!(msg, Msg::Tick) {
            self.tick();
            if self.page() == Page::PagerP {
                pager.update(&Msg::Tick); // follow re-pins to the new tail
                self.repaint = true;
            }
            return;
        }
        self.repaint = true;

        match self.page() {
            Page::Menu => match msg {
                Msg::Select => match page_for(self.menu.selected()) {
                    Some(p) => self.push(p),
                    None => self.quit = true,
                },
                _ => self.menu.update(msg),
            },

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
                    let mut f: [&mut dyn FormField; 4] =
                        [&mut self.counter, &mut self.slider, &mut self.picker, &mut self.back];
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

            Page::ListP => {
                let before = self.list.selected();
                if self.zone_nav(msg, before, |s| s.list.update(&Msg::Down), |s| s.list.selected())
                {
                    self.list.update(&Msg::Up);
                }
            }
            Page::TreeP => {
                let before = self.tree.selected();
                if self.zone_nav(msg, before, |s| s.tree.update(&Msg::Down), |s| s.tree.selected())
                {
                    self.tree.update(&Msg::Up);
                }
                if !self.on_back && matches!(msg, Msg::Select) {
                    self.tree.update(&Msg::Select);
                }
            }
            Page::TableP => {
                let before = self.table.selected();
                self.zone_nav(msg, before, |s| s.table.update(&Msg::Down), |s| s.table.selected());
                if matches!(msg, Msg::Up) && !self.on_back {
                    self.table.update(&Msg::Up);
                }
            }
            Page::RadioP => {
                let before = self.radio.cursor();
                self.zone_nav(msg, before, |s| s.radio.update(&Msg::Down), |s| s.radio.cursor());
                if !self.on_back {
                    match msg {
                        Msg::Up => self.radio.update(&Msg::Up),
                        Msg::Select => self.radio.update(&Msg::Select),
                        _ => {}
                    }
                }
            }
            Page::HelpP => {
                let before = self.help.offset();
                self.zone_nav(msg, before, |s| s.help.update(&Msg::Down), |s| s.help.offset());
                if matches!(msg, Msg::Up) && !self.on_back {
                    self.help.update(&Msg::Up);
                }
            }

            // Realtime Pager: scroll detaches follow; Down past the tail → Back.
            Page::PagerP => {
                let before = pager.offset();
                match msg {
                    Msg::Up => {
                        if self.on_back {
                            self.on_back = false;
                        } else {
                            pager.update(&Msg::Up);
                        }
                    }
                    Msg::Down => {
                        if !self.on_back {
                            pager.update(&Msg::Down);
                            if pager.offset() == before {
                                self.on_back = true;
                            }
                        }
                    }
                    Msg::Select if self.on_back => self.pop(),
                    _ => {}
                }
            }

            Page::Text => self.stack_nav(msg, 9),
            Page::Indicators => self.stack_nav(msg, 6),

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

            Page::BarChartP | Page::StatusBarP | Page::Layout => match msg {
                Msg::Up => self.on_back = false,
                Msg::Down => self.on_back = true,
                Msg::Select if self.on_back => self.pop(),
                _ => {}
            },

            Page::DialogP => {
                let before = self.dialog.selected();
                self.zone_nav(msg, before, |s| s.dialog.update(&Msg::Down), |s| s.dialog.selected());
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

    /// Shared "Down past the end → Back, Select on Back → pop" rule (see oled.rs).
    /// Returns `true` when the caller should still apply an `Up` to its widget.
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
                    true
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

    fn view(&self, target: &mut dyn RenderTarget, pager: &Pager<'_, StreamLog>) {
        let w = target.width();
        let h = target.height();
        let lh = target.line_height().max(1);
        if w == 0 || h < lh * 3 {
            return;
        }

        if self.force {
            target.clear(Area::new(0, 0, w, h));
        }

        let [head, mid, foot] = VStack::split(
            Area::new(0, 0, w, h),
            &[Length(lh + 2), Fill(1), Length(lh + 2)],
        );
        self.title.view(target, Area::new(head.x, head.y + 1, head.w, lh));

        // The status-bar hint is chrome: it only needs repainting on a transition
        // (otherwise it persists), which keeps idle/animation frames partial.
        if self.force {
            StatusBar::new().with_left(hint_for(self.page())).with_right("knurl").view(target, foot);
        }

        let body = Area::new(4, mid.y, mid.w.saturating_sub(8), mid.h);
        self.view_body(target, body, pager);
    }

    fn view_body(&self, target: &mut dyn RenderTarget, body: Area, pager: &Pager<'_, StreamLog>) {
        let lh = target.line_height().max(1);
        match self.page() {
            Page::Menu => self.menu.view(target, body),

            Page::Toggles => {
                let f: [&dyn FormField; 3] = [&self.chk, &self.tog, &self.back];
                self.form.view(target, body, &f);
            }
            Page::Editors => {
                let f: [&dyn FormField; 4] = [&self.counter, &self.slider, &self.picker, &self.back];
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
            Page::HelpP => self.body_with_back(target, body, |s, t, a| s.help.view(t, a)),
            Page::DialogP => self.body_with_back(target, body, |s, t, a| s.dialog.view(t, a)),

            Page::PagerP => {
                let [top, back] = VStack::split(body, &[Fill(1), Length(lh)]);
                pager.view(target, top);
                self.draw_back(target, back);
            }

            Page::BarChartP => self.body_with_back(target, body, |s, t, a| {
                let data = [
                    ("Cpu", s.chan[0]),
                    ("Mem", s.chan[1]),
                    ("Net", s.chan[2]),
                    ("Disk", s.chan[3]),
                ];
                BarChart::new(&data).with_label_width(48).with_max(100).view(t, a);
            }),

            Page::TabsP => self.body_with_back(target, body, |s, t, a| s.view_tabs(t, a)),
            Page::StatusBarP => self.body_with_back(target, body, |_s, t, a| {
                let lh = t.line_height();
                let row = Area::new(a.x, a.y, a.w, lh);
                StatusBar::new().with_left("Left").with_center("Center").with_right("Right").view(t, row);
                Label::new("StatusBar: left / center / right")
                    .with_style(Style::Muted)
                    .view(t, Area::new(a.x, a.y + lh * 2, a.w, lh));
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
                draw_stack(target, stack, self.scroll, &self.rows_indicators(), &self.spinner);
                self.draw_back(target, back);
            }
        }
    }

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
        let style = if self.on_back { Style::Focus } else { Style::Muted };
        Label::new("< Back").with_style(style).view(target, area);
    }

    fn view_tabs(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        self.tabs.view(target, Area::new(area.x, area.y, area.w, lh));
        let tab = self.tabs.selected().min(2);
        for (i, item) in TAB_CONTENT[tab].iter().enumerate() {
            let y = area.y + lh * 2 + i as u16 * lh;
            Label::new(item).with_style(Style::Normal).view(target, Area::new(area.x, y, area.w, lh));
        }
    }

    fn view_layout(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [top, mid, bot] = VStack::split(area, &[Length(lh), Fill(1), Length(lh)]);
        Label::new("VStack: top row").with_style(Style::Accent).view(target, top);
        let [l, r] = HStack::split(mid, &[Fill(1), Fill(1)]);
        target.draw_box(l, BorderStyle::Rounded);
        target.draw_box(r, BorderStyle::Rounded);
        Padded::new(Label::new("Left pane"), Padding::uniform(4)).view(target, l);
        Padded::new(Label::new("Right pane"), Padding::uniform(4)).view(target, r);
        Label::new("HStack + Bordered + Padded").with_style(Style::Accent).view(target, bot);
    }

    fn view_position(&self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [rows_area, pag_row] = VStack::split(area, &[Fill(1), Length(lh)]);
        let visible = ((rows_area.h / lh) as usize).clamp(1, POS_ROWS.len());
        self.vis.set(visible);
        for r in 0..visible {
            let idx = self.pos_offset + r;
            if idx >= POS_ROWS.len() {
                break;
            }
            let style = if r == 0 { Style::Focus } else { Style::Muted };
            Label::new(POS_ROWS[idx]).with_style(style).view(
                target,
                Area::new(rows_area.x, rows_area.y + r as u16 * lh, rows_area.w.saturating_sub(4), lh),
            );
        }
        let mut sb = Scrollbar::new();
        sb.set(POS_ROWS.len(), visible, self.pos_offset);
        sb.view(target, Area::new(rows_area.x + rows_area.w - 3, rows_area.y, 3, visible as u16 * lh));
        let pages = POS_ROWS.len() - visible + 1;
        Paginator::new(pages).with_current(self.pos_offset).view(target, pag_row);
    }

    fn rows_text(&self) -> [Row; 9] {
        [
            Row::Text("Normal text", Style::Normal),
            Row::Text("Accent text", Style::Accent),
            Row::Text("Muted text", Style::Muted),
            Row::Text("Danger text", Style::Danger),
            Row::Sep,
            Row::TitleC("Centered title"),
            Row::TitleR("Right title"),
            Row::Spacer,
            Row::Text("(spacer above)", Style::Muted),
        ]
    }

    fn rows_indicators(&self) -> [Row; 6] {
        [
            Row::Spin,
            Row::Text("ProgressBar", Style::Muted),
            Row::Bar(self.chan[0]),
            Row::Text("LineGauge", Style::Muted),
            Row::Gauge(self.gauge_v),
            Row::Text("live values", Style::Muted),
        ]
    }
}

fn main() {
    let mut sim = ColorSimulator::new(ColorSimConfig {
        title: "knurl TFT - 320x240 catalog (Up/Down, Space)".to_string(),
        ..Default::default()
    });

    // The streaming log + its Pager live here (not in Demo) so the Pager can
    // borrow the log without a self-referential struct - mirrors pager_stream.rs.
    let log = StreamLog::new(256);
    let mut ticks = 0u32;
    let mut count = 0u32;
    for _ in 0..8 {
        count += 1;
        log.push(format!("[{count:03}] sensor = {}", (count * 37) % 1000));
    }
    let mut pager = Pager::new(&log).with_follow(true);

    let mut demo = Demo::new();

    // Non-move closure: `pager` borrows `&log`; the body also appends to `log`
    // (`&self`) - both are shared borrows, so they coexist alongside `&mut demo`.
    sim.run_gated(|target, msgs| {
        for msg in msgs {
            if let Msg::Tick = msg {
                ticks += 1;
                if ticks.is_multiple_of(3) {
                    count += 1;
                    log.push(format!("[{count:03}] sensor = {}", (count * 37) % 1000));
                }
            }
            demo.handle(msg, &mut pager);
        }
        if demo.quit {
            return Frame::Quit;
        }
        if !core::mem::take(&mut demo.repaint) {
            return Frame::Skipped;
        }
        demo.view(target, &pager);
        demo.force = false;
        Frame::Painted
    });
}
