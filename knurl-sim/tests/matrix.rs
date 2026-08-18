//! The screenshot matrix: every widget, in every state, against a golden image.
//!
//! The nine pictures in `docs/` are a shop window - they show the demo, and
//! they change whenever the demo does. They have never been a test. Across six
//! steps of work, "the screenshots did not change" was said many times and
//! proved nothing: no scene in `docs/` contains a **focused** `Checkbox`, a
//! `Radio` under a cursor, `BorderStyle::Thick`, an editing `Slider`, or a tab
//! strip in either of its two focus states. Every phase ended with a temporary
//! example rendered by hand and deleted afterwards.
//!
//! This is that example, kept. One cell per widget-state, rendered headlessly
//! into an area the size of the widget, compared against
//! `knurl-sim/tests/matrix/<name>.png` **pixel by pixel** - never by PNG bytes,
//! because an encoder that changes its output between versions would turn the
//! whole matrix into a liar overnight.
//!
//! ## Regenerating
//!
//! ```sh
//! KNURL_MATRIX=bless cargo test -p knurl-sim --test matrix
//! ```
//!
//! Read the diff before committing it: every changed cell is a change to what
//! the library draws.
//!
//! ## When it fails
//!
//! Beside the golden that did not match you get `<name>.actual.png` (what was
//! drawn) and `<name>.diff.png` (differing pixels in magenta, matching ones
//! dimmed). Both are gitignored. Look at them; the assertion can only tell you
//! how many pixels moved.
//!
//! ## Determinism
//!
//! No clock and no randomness: an animation frame is reached by sending exactly
//! as many `Tick`s as the cell wants, and every widget is built fresh.

use std::path::{Path, PathBuf};

use embedded_graphics::{
    mono_font::ascii::FONT_6X10,
    pixelcolor::{BinaryColor, Rgb565, Rgb888},
    prelude::*,
};
use embedded_graphics_simulator::SimulatorDisplay;

use knurl::{
    Align, Area, BarChart, BorderStyle, Bordered, Button, Canvas, Checkbox, Component, Counter,
    Dialog, FocusZone, Form, FormField, Help, Label, LineGauge, List, Marker, Msg, Padded, Padding,
    Pager, Paginator, Picker, ProgressBar, Radio, RenderTarget, Screen, Scrollbar, Separator,
    Slider, Spinner, SpinnerStyle, StatusBar, Style, TabPages, Table, Tabs, TextInput, Title,
    Toggle, Tree, TreeItem,
};
use knurl_screens::{canvas::CanvasScreen, thermostat::ThermostatScreen};
use knurl_sim::graphics::{ColorGraphicsTarget, ColorTheme, GraphicsTarget, Theme};

// ── Demo data ────────────────────────────────────────────────────────────────

const ITEMS: &[&str] = &["Alpha", "Bravo", "Charlie", "Delta"];
const NONE: &[&str] = &[];
const NODES: &[TreeItem] = &[
    TreeItem::new("project", 0),
    TreeItem::new("src", 1),
    TreeItem::new("main", 2),
    TreeItem::new("docs", 0),
];
const ROWS: [[&str; 3]; 3] = [
    ["Bolt", "12", "3"],
    ["Nut", "34", "1"],
    ["Screw", "75", "4"],
];
const NO_ROWS: [[&str; 3]; 0] = [];
const WIDTHS: [u16; 3] = [54, 24, 18];
const HEADERS: &[&str] = &["Name", "Qty", "Pri"];
const BARS: [(&str, u16); 2] = [("Cpu", 30), ("Mem", 90)];
const KEYS: &[(&str, &str)] = &[("Turn", "move"), ("Push", "pick"), ("Hold", "menu")];
const LINES: &[&str] = &["[001] boot", "[002] ready", "[003] idle", "[004] busy"];
const MODES: &[&str] = &["Eco", "Balanced", "Turbo"];

// ── A cell ───────────────────────────────────────────────────────────────────

/// Which target a cell is rendered through. Most states are worth seeing on
/// both: a monochrome theme collapses several styles onto one ink, so a cue
/// that reads in colour can be invisible there - which is exactly the class of
/// bug this matrix exists to catch.
#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Mono,
    Colour,
}

struct Cell {
    name: &'static str,
    w: u16,
    h: u16,
    panel: Panel,
    draw: fn(&mut dyn RenderTarget, Area),
}

const fn mono(name: &'static str, w: u16, h: u16, draw: fn(&mut dyn RenderTarget, Area)) -> Cell {
    Cell {
        name,
        w,
        h,
        panel: Panel::Mono,
        draw,
    }
}

const fn colour(name: &'static str, w: u16, h: u16, draw: fn(&mut dyn RenderTarget, Area)) -> Cell {
    Cell {
        name,
        w,
        h,
        panel: Panel::Colour,
        draw,
    }
}

/// The same scene on both panels, named `<name>-mono` / `<name>-colour`.
macro_rules! both {
    ($cells:expr, $name:literal, $w:expr, $h:expr, $draw:expr) => {
        $cells.push(mono(concat!($name, "-mono"), $w, $h, $draw));
        $cells.push(colour(concat!($name, "-colour"), $w, $h, $draw));
    };
}

// ── The matrix ───────────────────────────────────────────────────────────────

fn cells() -> Vec<Cell> {
    let mut c: Vec<Cell> = Vec::new();

    // ── Indicators: the states no demo scene has ever contained ──────────
    both!(c, "checkbox-off", 90, 10, |t, a| {
        Checkbox::new("Logging").view(t, a)
    });
    both!(c, "checkbox-on", 90, 10, |t, a| {
        Checkbox::new("Logging").with_checked(true).view(t, a)
    });
    both!(c, "checkbox-off-focused", 90, 10, |t, a| {
        let mut w = Checkbox::new("Logging");
        w.focus();
        w.view(t, a);
    });
    both!(c, "checkbox-on-focused", 90, 10, |t, a| {
        let mut w = Checkbox::new("Logging").with_checked(true);
        w.focus();
        w.view(t, a);
    });
    both!(c, "toggle-off", 90, 10, |t, a| {
        Toggle::new("Wifi").view(t, a)
    });
    both!(c, "toggle-on", 90, 10, |t, a| {
        Toggle::new("Wifi").with_on(true).view(t, a)
    });
    both!(c, "toggle-on-focused", 90, 10, |t, a| {
        let mut w = Toggle::new("Wifi").with_on(true);
        w.focus();
        w.view(t, a);
    });
    both!(c, "radio-unfocused", 90, 30, |t, a| {
        let mut w = Radio::new(MODES);
        let _ = w.update(&Msg::Down);
        let _ = w.update(&Msg::Select);
        w.view(t, a);
    });
    both!(c, "radio-focused", 90, 30, |t, a| {
        let mut w = Radio::new(MODES);
        let _ = w.update(&Msg::Down);
        let _ = w.update(&Msg::Select);
        w.focus();
        w.view(t, a);
    });

    // ── The tab strip in both of its focus states ────────────────────────
    both!(c, "tabs-strip-focused", 120, 12, |t, a| {
        let mut w = Tabs::new(&["Net", "Log", "Sys"]);
        let _ = w.update(&Msg::Down);
        w.set_focused(true);
        w.view(t, a);
    });
    both!(c, "tabs-page-focused", 120, 12, |t, a| {
        let mut w = Tabs::new(&["Net", "Log", "Sys"]);
        let _ = w.update(&Msg::Down);
        w.set_focused(false);
        w.view(t, a);
    });

    // ── Value editors, in and out of the edit mode ───────────────────────
    both!(c, "slider-idle", 120, 10, |t, a| {
        let mut w = Slider::new("Level").with_range(0, 100).with_value(60);
        w.focus();
        w.view(t, a);
    });
    both!(c, "slider-editing", 120, 10, |t, a| {
        let mut w = Slider::new("Level").with_range(0, 100).with_value(60);
        w.focus();
        w.set_editing(true);
        w.view(t, a);
    });
    both!(c, "counter-idle", 120, 10, |t, a| {
        let mut w = Counter::new("Count").with_range(0, 20).with_value(7);
        w.focus();
        w.view(t, a);
    });
    both!(c, "counter-editing", 120, 10, |t, a| {
        let mut w = Counter::new("Count").with_range(0, 20).with_value(7);
        w.focus();
        w.set_editing(true);
        w.view(t, a);
    });
    both!(c, "picker-idle", 120, 10, |t, a| {
        let mut w = Picker::new("Mode", MODES);
        w.focus();
        w.view(t, a);
    });
    both!(c, "picker-editing", 120, 10, |t, a| {
        let mut w = Picker::new("Mode", MODES);
        w.focus();
        w.set_editing(true);
        w.view(t, a);
    });
    both!(c, "textinput-idle", 120, 10, |t, a| {
        let mut w = TextInput::<8>::new("Name");
        w.focus();
        w.view(t, a);
    });
    both!(c, "textinput-editing", 120, 22, |t, a| {
        let mut w = TextInput::<8>::new("Name");
        w.focus();
        w.set_editing(true);
        let _ = w.update(&Msg::Select);
        w.view(t, a);
    });

    // ── Borders, including one flush against the panel edge ──────────────
    c.push(mono("border-single", 90, 30, |t, a| {
        Bordered::new(Label::new("in"), BorderStyle::Single).view(t, a)
    }));
    c.push(mono("border-rounded", 90, 30, |t, a| {
        Bordered::new(Label::new("in"), BorderStyle::Rounded).view(t, a)
    }));
    c.push(mono("border-thick", 90, 30, |t, a| {
        Bordered::new(Label::new("in"), BorderStyle::Thick).view(t, a)
    }));
    c.push(mono("border-double", 90, 30, |t, a| {
        Bordered::new(Label::new("in"), BorderStyle::Double).view(t, a)
    }));
    c.push(colour("border-thick-colour", 90, 30, |t, a| {
        Bordered::new(Label::new("in"), BorderStyle::Thick).view(t, a)
    }));
    // Flush: the border occupies the outermost pixels of the panel, which is
    // where a stroke that overshoots by one would be clipped away unseen.
    c.push(mono("border-thick-flush", 40, 20, |t, a| {
        Bordered::new(Label::new("x"), BorderStyle::Thick).view(t, a)
    }));
    c.push(mono("border-double-tight", 20, 14, |t, a| {
        Bordered::new(Label::new(""), BorderStyle::Double).view(t, a)
    }));
    c.push(mono("padded", 90, 30, |t, a| {
        Padded::new(Label::new("inset"), Padding::uniform(4)).view(t, a)
    }));

    // ── Cursors, focused and not ─────────────────────────────────────────
    both!(c, "list-focused", 100, 30, |t, a| {
        let mut w = List::new(ITEMS);
        let _ = w.update(&Msg::Down);
        w.focus();
        w.view(t, a);
    });
    both!(c, "list-unfocused", 100, 30, |t, a| {
        let mut w = List::new(ITEMS);
        let _ = w.update(&Msg::Down);
        w.view(t, a);
    });
    c.push(mono("list-marker-none", 100, 30, |t, a| {
        let mut w = List::new(ITEMS).with_marker(Marker::NONE);
        let _ = w.update(&Msg::Down);
        w.focus();
        w.view(t, a);
    }));
    both!(c, "tree-focused", 110, 40, |t, a| {
        let mut w = Tree::new(NODES);
        let _ = w.update(&Msg::Select); // expand the root
        let _ = w.update(&Msg::Down);
        w.focus();
        w.view(t, a);
    });
    both!(c, "tree-unfocused", 110, 40, |t, a| {
        let mut w = Tree::new(NODES);
        let _ = w.update(&Msg::Select);
        let _ = w.update(&Msg::Down);
        w.view(t, a);
    });
    both!(c, "table-focused", 120, 40, |t, a| {
        let mut w = Table::new(&ROWS, &WIDTHS).with_headers(HEADERS);
        let _ = w.update(&Msg::Down);
        w.focus();
        w.view(t, a);
    });
    both!(c, "table-unfocused", 120, 40, |t, a| {
        let mut w = Table::new(&ROWS, &WIDTHS).with_headers(HEADERS);
        let _ = w.update(&Msg::Down);
        w.view(t, a);
    });

    // ── Scrolling: the indicator, and the widget it belongs to ───────────
    c.push(mono("list-scrolled", 100, 20, |t, a| {
        let mut w = List::new(ITEMS);
        for _ in 0..3 {
            let _ = w.update(&Msg::Down);
        }
        w.focus();
        w.view(t, a);
    }));
    c.push(mono("pager-follow", 100, 20, |t, a| {
        let mut w = Pager::new(LINES).with_follow(true);
        w.view(t, a); // the first frame caches the geometry
        let _ = w.update(&Msg::Tick);
        w.mark_dirty();
        w.view(t, a);
    }));
    c.push(mono("help", 120, 30, |t, a| Help::new(KEYS).view(t, a)));

    // ── Readouts ─────────────────────────────────────────────────────────
    both!(c, "barchart", 120, 20, |t, a| {
        BarChart::new(&BARS)
            .with_label_width(24)
            .with_max(100)
            .view(t, a)
    });
    c.push(mono("progressbar", 100, 10, |t, a| {
        let mut w = ProgressBar::new().with_max(100);
        w.set_value(40);
        w.view(t, a);
    }));
    c.push(mono("linegauge", 100, 10, |t, a| {
        let mut w = LineGauge::new().with_max(100);
        w.set_value(70);
        w.view(t, a);
    }));
    c.push(mono("scrollbar", 6, 40, |t, a| {
        let mut w = Scrollbar::new();
        w.set(10, 3, 4);
        w.view(t, a);
    }));
    c.push(mono("paginator-dots", 90, 10, |t, a| {
        Paginator::new(5).with_current(2).view(t, a)
    }));
    c.push(mono("paginator-numeric", 90, 10, |t, a| {
        Paginator::new(5)
            .with_numeric(true)
            .with_current(2)
            .view(t, a)
    }));
    c.push(mono("statusbar", 128, 14, |t, a| {
        StatusBar::new()
            .with_left("L")
            .with_center("CTR")
            .with_right("R")
            .view(t, a)
    }));

    // Spinner frames, reached by counting Ticks - the one place a matrix would
    // otherwise be tempted to read a clock.
    for (name, ticks) in [
        ("spinner-frame-0", 0u8),
        ("spinner-frame-1", 1),
        ("spinner-frame-2", 2),
        ("spinner-frame-3", 3),
    ] {
        c.push(Cell {
            name: Box::leak(name.to_string().into_boxed_str()),
            w: 60,
            h: 10,
            panel: Panel::Mono,
            draw: match ticks {
                0 => |t, a| spinner(t, a, 0),
                1 => |t, a| spinner(t, a, 1),
                2 => |t, a| spinner(t, a, 2),
                _ => |t, a| spinner(t, a, 3),
            },
        });
    }
    c.push(mono("spinner-braille", 60, 10, |t, a| {
        let mut w = Spinner::new().with_style(SpinnerStyle::Braille);
        let _ = w.update(&Msg::Tick);
        w.view(t, a);
    }));

    // ── Chrome and text ──────────────────────────────────────────────────
    c.push(mono("title-center", 120, 10, |t, a| {
        Title::new("Centred").with_align(Align::Center).view(t, a)
    }));
    c.push(mono("title-right", 120, 10, |t, a| {
        Title::new("Right").with_align(Align::Right).view(t, a)
    }));
    c.push(mono("separator", 90, 6, |t, a| Separator::new().view(t, a)));
    both!(c, "label-styles", 120, 50, |t, a| {
        let lh = t.line_height();
        for (i, style) in [
            Style::Normal,
            Style::Accent,
            Style::Muted,
            Style::Danger,
            Style::Inverted,
        ]
        .into_iter()
        .enumerate()
        {
            Label::new("Sample")
                .with_style(style)
                .view(t, Area::new(a.x, a.y + i as u16 * lh, a.w, lh));
        }
    });
    both!(c, "button-focused", 90, 10, |t, a| {
        let mut w = Button::new("< Back");
        w.focus();
        w.view(t, a);
    });

    // ── Dialogs, including the one with nothing to press ─────────────────
    both!(c, "dialog", 128, 50, |t, a| {
        Dialog::new("Erase?", "This cannot be undone", &["Ok", "Cancel"]).view(t, a)
    });
    c.push(mono("dialog-no-buttons", 128, 50, |t, a| {
        Dialog::new("Working", "Please wait", &[]).view(t, a)
    }));

    // ── Empty data ───────────────────────────────────────────────────────
    c.push(mono("list-empty", 100, 30, |t, a| {
        List::new(NONE).view(t, a)
    }));
    c.push(mono("table-empty", 120, 30, |t, a| {
        Table::new(&NO_ROWS, &WIDTHS)
            .with_headers(HEADERS)
            .view(t, a)
    }));
    c.push(mono("radio-empty", 100, 30, |t, a| {
        Radio::new(NONE).view(t, a)
    }));
    c.push(mono("tabs-empty", 120, 12, |t, a| {
        Tabs::new(NONE).view(t, a)
    }));

    // ── Tiny areas: 1-4px on each axis, where Phase 1 found its panics ───
    for (name, w, h) in [
        ("tiny-list-1x20", 1u16, 20u16),
        ("tiny-list-20x1", 20, 1),
        ("tiny-list-3x3", 3, 3),
        ("tiny-list-4x12", 4, 12),
        ("tiny-checkbox-2x10", 2, 10),
        ("tiny-dialog-4x4", 4, 4),
        ("tiny-help-3x20", 3, 20),
        ("tiny-table-3x30", 3, 30),
    ] {
        c.push(Cell {
            name: Box::leak(name.to_string().into_boxed_str()),
            w: w.max(1),
            h: h.max(1),
            panel: Panel::Mono,
            draw: match name {
                "tiny-checkbox-2x10" => |t, a| Checkbox::new("On").view(t, a),
                "tiny-dialog-4x4" => |t, a| Dialog::new("T", "m", &["Ok"]).view(t, a),
                "tiny-help-3x20" => |t, a| Help::new(KEYS).view(t, a),
                "tiny-table-3x30" => |t, a| Table::new(&ROWS, &WIDTHS).view(t, a),
                _ => |t, a| {
                    let mut w = List::new(ITEMS);
                    w.focus();
                    w.view(t, a);
                },
            },
        });
    }

    // ── A form, the controller everything else composes through ──────────
    both!(c, "form", 120, 40, |t, a| {
        let mut check = Checkbox::new("Log");
        let mut slider = Slider::new("Lvl").with_range(0, 100).with_value(30);
        let mut back = Button::new("< Back");
        let mut form = Form::new();
        let mut fields: [&mut dyn FormField; 3] = [&mut check, &mut slider, &mut back];
        let _ = form.update(&Msg::Down, &mut fields);
        form.view(t, a, &fields);
    });
    c.push(mono("tabpages-in-page", 120, 40, |t, a| {
        let mut tabs = Tabs::new(&["Net", "Log"]);
        let mut list = List::new(ITEMS);
        let mut pages = TabPages::new();
        {
            let mut zone = pages.zone(&mut tabs, &mut list);
            let _ = zone.handle(&Msg::Select); // into the page
        }
        let lh = t.line_height();
        tabs.view(t, Area::new(a.x, a.y, a.w, lh + 2));
        list.view(t, Area::new(a.x, a.y + lh + 2, a.w, a.h - lh - 2));
    }));

    // ── Free-hand drawing, and the widget written outside the library ────
    c.push(mono("canvas-crosshair", 60, 30, |t, a| {
        Canvas::new(|t: &mut dyn RenderTarget, a: Area| {
            t.draw_rect(a, Style::Muted);
            t.draw_line(a.x, a.y, a.x + a.w - 1, a.y + a.h - 1, Style::Accent);
            t.draw_line(a.x, a.y + a.h - 1, a.x + a.w - 1, a.y, Style::Accent);
        })
        .view(t, a)
    }));
    both!(c, "thermostat-idle", 120, 24, |t, a| {
        let mut s = ThermostatScreen::new();
        s.enter();
        s.view(t, Area::new(a.x, a.y, a.w, a.h + 20));
    });
    both!(c, "thermostat-warm", 120, 24, |t, a| {
        let mut s = ThermostatScreen::new();
        s.enter();
        for _ in 0..8 {
            s.update(&Msg::Up);
        }
        s.update(&Msg::Select);
        s.update(&Msg::Down);
        s.view(t, Area::new(a.x, a.y, a.w, a.h + 20));
    });
    c.push(colour("canvas-screen", 128, 54, |t, a| {
        let mut s = CanvasScreen::new();
        s.enter();
        s.view(t, a);
    }));

    c
}

/// A spinner advanced by exactly `ticks` steps - no clock anywhere.
fn spinner(t: &mut dyn RenderTarget, a: Area, ticks: u8) {
    let mut w = Spinner::new().with_label("live");
    for _ in 0..ticks {
        let _ = w.update(&Msg::Tick);
    }
    w.view(t, a);
}

// ── Rendering a cell to RGB ──────────────────────────────────────────────────

/// The cell's pixels, row-major RGB8. Read straight out of the display rather
/// than through an encoder, so what is compared is what was drawn.
fn render(cell: &Cell) -> Vec<u8> {
    let (w, h) = (cell.w as u32, cell.h as u32);
    let area = Area::new(0, 0, cell.w, cell.h);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);

    match cell.panel {
        Panel::Mono => {
            let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(w, h));
            display.clear(BinaryColor::Off).unwrap();
            {
                let mut target =
                    GraphicsTarget::new(&mut display, FONT_6X10).with_theme(Theme::new());
                (cell.draw)(&mut target, area);
            }
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let on = display.get_pixel(Point::new(x, y)) == BinaryColor::On;
                    rgb.extend_from_slice(&if on { [255, 255, 255] } else { [0, 0, 0] });
                }
            }
        }
        Panel::Colour => {
            let theme = ColorTheme::default();
            let mut display = SimulatorDisplay::<Rgb565>::new(Size::new(w, h));
            display.clear(theme.background(Style::Normal)).unwrap();
            {
                let mut target =
                    ColorGraphicsTarget::new(&mut display, FONT_6X10).with_theme(theme);
                (cell.draw)(&mut target, area);
            }
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let c = Rgb888::from(display.get_pixel(Point::new(x, y)));
                    rgb.extend_from_slice(&[c.r(), c.g(), c.b()]);
                }
            }
        }
    }
    rgb
}

// ── Golden files ─────────────────────────────────────────────────────────────

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/matrix")
}

fn write_png(path: &Path, rgb: &[u8], w: u32, h: u32) {
    image::save_buffer(path, rgb, w, h, image::ExtendedColorType::Rgb8)
        .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// Differing pixels in magenta, matching ones dimmed - so the eye finds the
/// change instead of hunting for it.
fn diff_png(path: &Path, want: &[u8], got: &[u8], w: u32, h: u32) {
    let mut out = Vec::with_capacity(want.len());
    for i in (0..want.len()).step_by(3) {
        if want[i..i + 3] == got[i..i + 3] {
            out.extend_from_slice(&[want[i] / 4, want[i + 1] / 4, want[i + 2] / 4]);
        } else {
            out.extend_from_slice(&[255, 0, 255]);
        }
    }
    write_png(path, &out, w, h);
}

// ── The tests ────────────────────────────────────────────────────────────────

#[test]
fn every_widget_state_matches_its_golden() {
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let bless = std::env::var("KNURL_MATRIX").as_deref() == Ok("bless");

    let cells = cells();
    let mut failures: Vec<String> = Vec::new();
    let mut blessed = 0usize;

    for cell in &cells {
        let got = render(cell);
        let path = dir.join(format!("{}.png", cell.name));
        let (w, h) = (cell.w as u32, cell.h as u32);

        if bless {
            write_png(&path, &got, w, h);
            blessed += 1;
            continue;
        }

        let Ok(golden) = image::open(&path) else {
            failures.push(format!(
                "{}: no golden. Run KNURL_MATRIX=bless cargo test -p knurl-sim --test matrix",
                cell.name
            ));
            write_png(&dir.join(format!("{}.actual.png", cell.name)), &got, w, h);
            continue;
        };
        let golden = golden.to_rgb8();
        if golden.dimensions() != (w, h) {
            failures.push(format!(
                "{}: golden is {:?}, the cell is {w}x{h}",
                cell.name,
                golden.dimensions()
            ));
            write_png(&dir.join(format!("{}.actual.png", cell.name)), &got, w, h);
            continue;
        }

        let want = golden.into_raw();
        let moved = want
            .chunks_exact(3)
            .zip(got.chunks_exact(3))
            .filter(|(a, b)| a != b)
            .count();
        if moved > 0 {
            write_png(&dir.join(format!("{}.actual.png", cell.name)), &got, w, h);
            diff_png(
                &dir.join(format!("{}.diff.png", cell.name)),
                &want,
                &got,
                w,
                h,
            );
            failures.push(format!(
                "{}: {moved} of {} pixels moved (see {}.actual.png / {}.diff.png)",
                cell.name,
                w * h,
                cell.name,
                cell.name
            ));
        }
    }

    if bless {
        println!("blessed {blessed} cells in {}", dir.display());
        return;
    }
    assert!(
        failures.is_empty(),
        "{} of {} cells differ:\n  {}",
        failures.len(),
        cells.len(),
        failures.join("\n  ")
    );
}

/// Two cells with the same name would silently shadow each other's golden, and
/// the matrix would quietly stop checking one of them.
#[test]
fn every_cell_has_its_own_name() {
    let cells = cells();
    let mut names: Vec<&str> = cells.iter().map(|c| c.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate cell name");
    assert!(before >= 60, "the matrix has shrunk to {before} cells");
}
