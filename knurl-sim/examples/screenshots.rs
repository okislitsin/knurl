//! Headless screenshot generator for the README - **no SDL window**.
//!
//! Each screen is composed from the same widgets, theme and chrome as the
//! `oled` / `tft` demos, rendered into an off-screen `SimulatorDisplay`, and
//! saved as a PNG via `to_rgb_output_image().save_png()`. That path never opens
//! an SDL window (no `Window::new`), so this runs in CI / over SSH.
//!
//! Run from the workspace root:
//! ```sh
//! cargo run -p knurl-sim --features desktop --example screenshots
//! ```
//! Output lands in `docs/` (paths are resolved relative to the crate, not the
//! current directory).

use embedded_graphics::{
    mono_font::ascii::FONT_6X10,
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
};
use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};

use knurl_sim::core::{
    Align, Area, BarChart, Button, Component,
    Constraint::{Fill, Length},
    Counter, Dialog, Form, FormField, List, Msg, Pager, Picker, RenderTarget, Slider, StatusBar,
    Style, Table, Title, Tree, TreeItem, VStack,
};
use knurl_sim::graphics::{ColorGraphicsTarget, ColorTheme, GraphicsTarget, Theme};

// ── Demo data (ASCII), mirroring the oled / tft demos ──────────────────────────

const OLED_MENU: &[&str] = &[
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
const TFT_MENU: &[&str] = &[
    "Text",
    "List",
    "Tree",
    "Table",
    "Bar chart",
    "Toggles",
    "Editors",
    "Radio",
    "Text input",
    "Pager (live)",
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
    TreeItem::new("project", 0),
    TreeItem::new("src", 1),
    TreeItem::new("main", 2),
    TreeItem::new("lib", 2),
    TreeItem::new("docs", 1),
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
const PAGER_LINES: &[&str] = &[
    "[001] sensor = 37",
    "[002] sensor = 74",
    "[003] sensor = 111",
    "[004] sensor = 148",
    "[005] sensor = 185",
    "[006] sensor = 222",
    "[007] sensor = 259",
    "[008] sensor = 296",
];

// ── Headless shooters ──────────────────────────────────────────────────────────

fn docs_path(name: &str) -> String {
    format!("{}/../docs/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Renders a mono (`BinaryColor`) screen to an off-screen display and saves a PNG.
fn shoot_mono(w: u32, h: u32, scale: u32, name: &str, draw: impl FnOnce(&mut dyn RenderTarget)) {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(w, h));
    display.clear(BinaryColor::Off).unwrap();
    {
        let mut target = GraphicsTarget::new(&mut display, FONT_6X10).with_theme(Theme::new());
        draw(&mut target);
    }
    let output = OutputSettingsBuilder::new().scale(scale).build();
    display
        .to_rgb_output_image(&output)
        .save_png(docs_path(name))
        .unwrap();
    println!("wrote docs/{name}");
}

/// Renders a colour (`Rgb565`) screen with the Charm `ColorTheme` and saves a PNG.
fn shoot_color(w: u32, h: u32, scale: u32, name: &str, draw: impl FnOnce(&mut dyn RenderTarget)) {
    let theme = ColorTheme::default();
    let mut display = SimulatorDisplay::<Rgb565>::new(Size::new(w, h));
    display.clear(theme.background(Style::Normal)).unwrap();
    {
        let mut target = ColorGraphicsTarget::new(&mut display, FONT_6X10).with_theme(theme);
        draw(&mut target);
    }
    let output = OutputSettingsBuilder::new().scale(scale).build();
    display
        .to_rgb_output_image(&output)
        .save_png(docs_path(name))
        .unwrap();
    println!("wrote docs/{name}");
}

// ── Chrome helpers (mirror the demos) ──────────────────────────────────────────

/// Draws a centred title and returns the body area below it (OLED: no status bar).
fn oled_chrome(t: &mut dyn RenderTarget, title: &str) -> Area {
    let (w, h, lh) = (t.width(), t.height(), t.line_height());
    Title::new(title)
        .with_align(Align::Center)
        .view(t, Area::new(0, 0, w, lh));
    Area::new(1, lh, w.saturating_sub(1), h.saturating_sub(lh))
}

/// Draws a centred title + a bottom status-bar hint, returns the body between.
fn tft_chrome(t: &mut dyn RenderTarget, title: &str, hint: &str) -> Area {
    let (w, h, lh) = (t.width(), t.height(), t.line_height());
    let [head, mid, foot] = VStack::split(
        Area::new(0, 0, w, h),
        &[Length(lh + 2), Fill(1), Length(lh + 2)],
    );
    Title::new(title)
        .with_align(Align::Center)
        .view(t, Area::new(head.x, head.y + 1, head.w, lh));
    StatusBar::new()
        .with_left(hint)
        .with_right("knurl")
        .view(t, foot);
    Area::new(4, mid.y, mid.w.saturating_sub(8), mid.h)
}

/// Splits a body into [widget, back-row] and draws the muted "< Back" item.
fn body_with_back(t: &mut dyn RenderTarget, body: Area) -> Area {
    let lh = t.line_height();
    let [top, back] = VStack::split(body, &[Fill(1), Length(lh)]);
    Title::new("< Back").with_style(Style::Muted).view(t, back);
    top
}

fn main() {
    std::fs::create_dir_all(format!("{}/../docs", env!("CARGO_MANIFEST_DIR"))).ok();

    // ── OLED (128x64 mono) ──────────────────────────────────────────────────

    // Root menu - the catalog, with the built-in scroll indicator.
    let menu = List::new(OLED_MENU);
    shoot_mono(128, 64, 4, "oled-menu.png", |t| {
        let body = oled_chrome(t, "knurl OLED");
        menu.view(t, body);
    });

    // List page - selection driven down a few rows.
    let mut list = List::new(LIST_ITEMS);
    list.update(&Msg::Down);
    list.update(&Msg::Down);
    shoot_mono(128, 64, 4, "oled-list.png", |t| {
        let body = oled_chrome(t, "List");
        let top = body_with_back(t, body);
        list.view(t, top);
    });

    // Tree page - expand the root and "src" to show nesting.
    let mut tree = Tree::new(TREE_ITEMS);
    tree.update(&Msg::Select); // expand "project"
    tree.update(&Msg::Down);
    tree.update(&Msg::Select); // expand "src"
    shoot_mono(128, 64, 4, "oled-tree.png", |t| {
        let body = oled_chrome(t, "Tree");
        let top = body_with_back(t, body);
        tree.view(t, top);
    });

    // Editors form - Counter / Slider / Picker + a focusable Back button.
    let mut counter = Counter::new("Bright")
        .with_range(0, 100)
        .with_step(10)
        .with_value(60);
    let mut slider = Slider::new("Vol")
        .with_range(0, 100)
        .with_step(10)
        .with_value(40);
    let mut picker = Picker::new("Mode", MODES);
    let mut back = Button::new("< Back");
    let form = Form::new();
    {
        let mut f: [&mut dyn FormField; 4] = [&mut counter, &mut slider, &mut picker, &mut back];
        form.sync_focus(&mut f);
    }
    shoot_mono(128, 64, 4, "oled-form.png", |t| {
        let body = oled_chrome(t, "Editors");
        let f: [&dyn FormField; 4] = [&counter, &slider, &picker, &back];
        form.view(t, body, &f);
    });

    // ── TFT (320x240 colour) ────────────────────────────────────────────────

    let tmenu = List::new(TFT_MENU);
    shoot_color(320, 240, 2, "tft-menu.png", |t| {
        let body = tft_chrome(t, "knurl TFT catalog", "Turn: move   Push: open");
        tmenu.view(t, body);
    });

    let mut table = Table::new(&TABLE_ROWS, &TABLE_W).with_headers(TABLE_HEADERS);
    table.update(&Msg::Down);
    shoot_color(320, 240, 2, "tft-table.png", |t| {
        let body = tft_chrome(t, "Table", "Turn: move   Push: Back");
        let top = body_with_back(t, body);
        table.view(t, top);
    });

    let chart_data = [("Cpu", 72u16), ("Mem", 45), ("Net", 88), ("Disk", 30)];
    shoot_color(320, 240, 2, "tft-chart.png", |t| {
        let body = tft_chrome(t, "Bar chart", "Turn: move   Push: Back");
        let top = body_with_back(t, body);
        BarChart::new(&chart_data)
            .with_label_width(48)
            .with_max(100)
            .view(t, top);
    });

    let pager = Pager::new(PAGER_LINES).with_follow(true);
    shoot_color(320, 240, 2, "tft-pager.png", |t| {
        let body = tft_chrome(t, "Pager (live stream)", "Turn: scroll, bottom = follow");
        let top = body_with_back(t, body);
        pager.view(t, top);
    });

    let dialog = Dialog::new(
        "Save changes?",
        "Apply the new settings?",
        &["OK", "Cancel"],
    );
    shoot_color(320, 240, 2, "tft-dialog.png", |t| {
        let body = tft_chrome(t, "Dialog", "Push: a button / Back");
        let top = body_with_back(t, body);
        dialog.view(t, top);
    });

    println!("done - screenshots in docs/");
}
