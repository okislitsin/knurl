//! A seeded smoke run over every widget and every composition.
//!
//! The per-widget tests each check a thing somebody thought of. This checks the
//! thing nobody did: a random walk of events over a random area, including the
//! areas a layout produces when it runs out of room - zero, one pixel, three.
//! Phase 1 found its panics exactly there, and nothing but a sweep finds the
//! next one.
//!
//! It is deterministic. The generator is ten lines of xorshift seeded from the
//! case number, so a failure names a seed and the seed reproduces it - print it
//! in every assertion message, because a fuzz failure nobody can re-run is a
//! flake.
//!
//! **Run it in debug.** Half of what it looks for is `attempt to subtract with
//! overflow`, and release builds wrap silently instead - a `--release` sweep is
//! a sweep that finds nothing. `cargo test` is a debug profile, so the default
//! is the right one; raise the loop counts below if you want a longer hunt
//! (300 000 seeds is about a second).
//!
//! What it asserts, beyond "does not panic":
//!
//! * **nothing painted means the paint is still owed** - the [`min_size`]
//!   contract. A widget that drew nothing and went clean is a widget that will
//!   stay blank for as long as it lives;
//! * **the region never leaves the panel**, whatever a widget was asked to
//!   paint into. That is what an application hands to a display driver
//!   unchecked.
//!
//! "A widget paints inside its own area" is **not** among them, and that is a
//! finding rather than an omission: squeeze almost any widget below the width
//! of its content and it paints past the edge, because widgets lay out in
//! pixels and leave clipping to the target - which clips at the panel, not at
//! the widget. See `a_squeezed_widget_paints_outside_its_own_area` at the foot
//! of this file, which pins the case with numbers.
//!
//! [`min_size`]: crate::Component::min_size

use crate::mock::RecordingTarget;
use crate::*;

// ── The generator ────────────────────────────────────────────────────────────

/// xorshift32: ten lines, no dependency, and the same sequence everywhere.
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        // 0 is a fixed point of xorshift, so it is not a usable seed.
        Self(seed | 1)
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }

    /// An area from the awkward end of the range: zero and one-to-three pixels
    /// are what a layout hands over when it has run out, and they are where the
    /// arithmetic bugs live.
    fn area(&mut self) -> Area {
        let dim = |r: &mut Self| match r.below(4) {
            0 => 0,
            1 => 1 + r.below(3),    // 1..=3
            2 => 4 + r.below(28),   // a squeezed widget
            _ => 20 + r.below(100), // room to work
        };
        let x = self.below(8) as u16;
        let y = self.below(8) as u16;
        Area::new(x, y, dim(self) as u16, dim(self) as u16)
    }

    fn msg(&mut self) -> Msg {
        match self.below(8) {
            0 => Msg::Up,
            1 => Msg::Down,
            2 => Msg::Select,
            3 => Msg::Tick,
            4 => Msg::Left,
            5 => Msg::Right,
            6 => Msg::Back,
            _ => Msg::Char(char::from(b'a' + self.below(26) as u8)),
        }
    }
}

// ── The invariants ───────────────────────────────────────────────────────────

/// Paints `c` into `area` on a fresh target and checks what came of it.
fn paint(c: &dyn Component, area: Area, what: &str, seed: u32) {
    let mut t = RecordingTarget::new(160, 140);
    let was_dirty = c.dirty();
    c.view(&mut t, area);

    if let Some(region) = t.take_dirty_rect() {
        assert!(
            region.x as u32 + region.w as u32 <= 160 && region.y as u32 + region.h as u32 <= 140,
            "{what} reported {region:?}, which runs off the panel (seed {seed})"
        );
    } else if was_dirty {
        assert!(
            c.dirty(),
            "{what} painted nothing into {area:?} and went clean anyway (seed {seed})"
        );
    }
}

// ── Widgets ──────────────────────────────────────────────────────────────────

/// Every widget in the catalogue, driven and painted. The `match` builds one
/// per case because they have no common type - and because a widget rebuilt per
/// case is a widget whose first frame is exercised too.
fn drive(kind: u32, rng: &mut Rng, seed: u32) {
    const ITEMS: &[&str] = &["Alpha", "Beta", "Gamma", "Delta"];
    const NODES: &[TreeItem] = &[
        TreeItem::new("root", 0),
        TreeItem::new("child", 1),
        TreeItem::new("leaf", 2),
        TreeItem::new("other", 0),
    ];
    const ROWS: &[[&str; 3]] = &[["a", "1", "x"], ["b", "2", "y"], ["c", "3", "z"]];
    const WIDTHS: &[u16] = &[40, 20, 20];
    const BARS: &[(&str, u16)] = &[("Cpu", 30), ("Mem", 90)];
    const KEYS: &[(&str, &str)] = &[("Up", "move"), ("Sel", "pick")];
    const EMPTY_ITEMS: &[&str] = &[];
    const EMPTY_ROWS: &[[&str; 3]] = &[];

    /// The event walk and the paints, over whatever was built.
    macro_rules! run {
        ($name:expr, $w:expr) => {{
            let mut w = $w;
            let name = $name;
            for _ in 0..12 {
                match rng.below(10) {
                    0 => w.focus(),
                    1 => w.blur(),
                    2 => w.mark_dirty(),
                    _ => {
                        let _ = w.update(&rng.msg());
                    }
                }
                paint(&w, rng.area(), name, seed);
            }
        }};
    }

    match kind {
        0 => run!("Label", Label::new("text")),
        1 => run!("Title", Title::new("title")),
        2 => run!("Separator", Separator::new()),
        3 => run!("Spacer", Spacer::new()),
        4 => run!("List", List::new(ITEMS)),
        5 => run!("List (empty)", List::new(EMPTY_ITEMS)),
        6 => run!("Tree", Tree::new(NODES)),
        7 => run!(
            "Table",
            Table::new(ROWS, WIDTHS).with_headers(&["A", "B", "C"])
        ),
        8 => run!("Table (empty)", Table::new(EMPTY_ROWS, WIDTHS)),
        9 => run!("BarChart", BarChart::new(BARS)),
        10 => run!("Pager", Pager::new(ITEMS).with_follow(true)),
        11 => run!("Help", Help::new(KEYS)),
        12 => run!("Dialog", Dialog::new("T", "message", &["Ok", "Cancel"])),
        13 => run!("Dialog (no buttons)", Dialog::new("T", "message", &[])),
        14 => run!("Tabs", Tabs::new(&["One", "Two", "Three"])),
        15 => run!("StatusBar", StatusBar::new().with_left("L").with_right("R")),
        16 => run!("Checkbox", Checkbox::new("Check")),
        17 => run!("Toggle", Toggle::new("Toggle")),
        18 => run!("Counter", Counter::new("Count").with_range(0, 5)),
        19 => run!("Slider", Slider::new("Slide").with_range(0, 100)),
        20 => run!("Picker", Picker::new("Pick", ITEMS)),
        21 => run!("Radio", Radio::new(ITEMS)),
        22 => run!("Radio (empty)", Radio::new(EMPTY_ITEMS)),
        23 => run!("TextInput", TextInput::<16>::new("Name")),
        24 => run!("Button", Button::new("Press")),
        25 => run!("Spinner", Spinner::new().with_label("live")),
        26 => run!("ProgressBar", ProgressBar::new().with_max(100)),
        27 => run!("LineGauge", LineGauge::new().with_max(100)),
        28 => run!("Scrollbar", Scrollbar::new()),
        29 => run!("Paginator", Paginator::new(4)),
        30 => run!(
            "Canvas",
            Canvas::new(|t: &mut dyn RenderTarget, a: Area| {
                t.draw_rect(a, Style::Accent);
                let (x1, y1) = (a.x + a.w.saturating_sub(1), a.y + a.h.saturating_sub(1));
                t.draw_line(a.x, a.y, x1, y1, Style::Muted);
            })
        ),
        31 => run!(
            "Bordered",
            Bordered::new(List::new(ITEMS), BorderStyle::Thick)
        ),
        _ => run!("Padded", Padded::new(Label::new("in"), Padding::uniform(2))),
    }
}

/// How many arms `drive` has.
const WIDGET_KINDS: u32 = 33;

// ── Compositions ─────────────────────────────────────────────────────────────

/// A form of a random subset of fields, driven through its controller. The
/// field set changes between rounds, which is the case that broke `Form` in
/// Step 1 and the one no per-widget test reaches.
fn drive_form(rng: &mut Rng, seed: u32) {
    let mut check = Checkbox::new("Check");
    let mut toggle = Toggle::new("Toggle");
    let mut counter = Counter::new("Count").with_range(0, 3);
    let mut slider = Slider::new("Slide").with_range(0, 10);
    let mut text = TextInput::<8>::new("Name");
    let mut form = Form::new();

    for _ in 0..16 {
        let msg = rng.msg();
        let area = rng.area();
        // A different set each round, including the empty one.
        match rng.below(4) {
            0 => {
                let mut fields: [&mut dyn FormField; 0] = [];
                let _ = form.update(&msg, &mut fields);
                form.view(&mut RecordingTarget::new(160, 140), area, &fields);
            }
            1 => {
                let mut fields: [&mut dyn FormField; 1] = [&mut check];
                let _ = form.update(&msg, &mut fields);
                form.view(&mut RecordingTarget::new(160, 140), area, &fields);
            }
            2 => {
                let mut fields: [&mut dyn FormField; 3] = [&mut check, &mut counter, &mut slider];
                let _ = form.update(&msg, &mut fields);
                form.view(&mut RecordingTarget::new(160, 140), area, &fields);
            }
            _ => {
                let mut fields: [&mut dyn FormField; 5] = [
                    &mut check,
                    &mut toggle,
                    &mut counter,
                    &mut slider,
                    &mut text,
                ];
                let _ = form.update(&msg, &mut fields);
                form.view(&mut RecordingTarget::new(160, 140), area, &fields);
            }
        }
        assert!(
            form.focus_index() < 5,
            "form focus ran off its own set (seed {seed})"
        );
    }
}

/// A chain of random zones, including the ones that trap the cursor and the
/// ones that refuse it. Routing is what a per-widget test cannot see.
fn drive_chain(rng: &mut Rng, seed: u32) {
    let mut list = List::new(&["a", "b", "c"]);
    let mut label = Label::new("chrome");
    let mut button = Button::new("< Back");
    let mut radio = Radio::new(&["x", "y"]);
    let mut scroll_at = 0usize;
    let mut chain = FocusChain::new();

    for _ in 0..24 {
        let msg = rng.msg();
        let mut scroll = ScrollZone::new(&mut scroll_at, rng.below(4) as usize);
        let mut none = NoZone;
        // The zone list itself varies: a screen may hand over a different one
        // every event (that is what makes `FocusChain` own no zones).
        match rng.below(3) {
            0 => {
                let zones: &mut [&mut dyn FocusZone] = &mut [&mut list, &mut button];
                let _ = chain.update(&msg, zones);
                chain.sync_focus(zones);
            }
            1 => {
                let zones: &mut [&mut dyn FocusZone] =
                    &mut [&mut label, &mut none, &mut scroll, &mut button];
                let _ = chain.update(&msg, zones);
                chain.sync_focus(zones);
            }
            _ => {
                let zones: &mut [&mut dyn FocusZone] = &mut [&mut radio];
                let _ = chain.update(&msg, zones);
                chain.sync_focus(zones);
            }
        }
        assert!(
            chain.focus_index() < 4,
            "chain focus ran off its own list (seed {seed})"
        );
    }
}

/// Tabs with live pages: the strip and the page are one zone in two modes, and
/// the switch between them is where the cursor gets lost.
fn drive_tabs(rng: &mut Rng, seed: u32) {
    let mut tabs = Tabs::new(&["One", "Two"]);
    let mut pages = TabPages::new();
    let mut check = Checkbox::new("Check");
    let mut list = List::new(&["a", "b"]);

    for _ in 0..20 {
        let msg = rng.msg();
        {
            let mut zone = if rng.below(2) == 0 {
                pages.zone(&mut tabs, &mut check)
            } else {
                pages.zone(&mut tabs, &mut list)
            };
            let _ = zone.handle(&msg);
        }
        let area = rng.area();
        let mut t = RecordingTarget::new(160, 140);
        tabs.view(&mut t, area);
        paint(&check, rng.area(), "Tabs page", seed);
    }
}

// ── The runs ─────────────────────────────────────────────────────────────────

/// Every widget, many seeds. Cheap enough to run on every `cargo test`: the
/// whole sweep is a few thousand paints against a recording target.
#[test]
fn no_widget_panics_on_a_random_walk() {
    for seed in 1..=20_000u32 {
        let mut rng = Rng::new(seed.wrapping_mul(2_654_435_761));
        drive(seed % WIDGET_KINDS, &mut rng, seed);
    }
}

/// ...and every widget gets its own seeds, so a rare arm is not left to chance.
#[test]
fn every_widget_kind_is_walked() {
    for kind in 0..WIDGET_KINDS {
        for round in 0..500u32 {
            let seed = kind * 100 + round + 1;
            let mut rng = Rng::new(seed.wrapping_mul(2_246_822_519));
            drive(kind, &mut rng, seed);
        }
    }
}

#[test]
fn no_composition_panics_on_a_random_walk() {
    for seed in 1..=4_000u32 {
        let mut rng = Rng::new(seed.wrapping_mul(2_654_435_761));
        drive_form(&mut rng, seed);
        drive_chain(&mut rng, seed);
        drive_tabs(&mut rng, seed);
    }
}

// ── What the sweep found ─────────────────────────────────────────────────────

/// A widget narrower than its content paints **past its own edge**, and the
/// region says so - the pixels would go to the panel, over whatever the layout
/// put to the right.
///
/// This is not an oversight in one widget: the sweep above hits it in 25 of the
/// 26 it drives. Widgets lay out in pixels and leave clipping to the target,
/// and the target clips at the panel. The ones that truncate *text*
/// ([`Title`], [`List`], [`Table`], [`Help`]) still draw their fixed furniture
/// - a marker column, an indicator square, a spinner glyph - at full size.
///
/// It is pinned rather than fixed because the fix is the horizontal-overflow
/// question the project has deliberately left open (what should a squeezed
/// widget *show*, not merely what should it avoid). This test is here so the
/// day somebody answers it, the answer is visible: it will fail, and the
/// numbers below say by how much a caller was being overdrawn.
#[test]
fn a_squeezed_widget_paints_outside_its_own_area() {
    // FONT metrics of the recording target: 6px per character, 10px a row.
    let area = Area::new(0, 0, 18, 10); // three characters wide
    let label = Label::new("far too long for this");
    let mut t = RecordingTarget::new(160, 140);
    label.view(&mut t, area);
    let region = t.take_dirty_rect().expect("it painted");
    assert_eq!(region.w, 126, "21 characters, in an 18px area");

    // A checkbox is squeezed differently: its indicator is a fixed square.
    let check = Checkbox::new("On");
    let mut t = RecordingTarget::new(160, 140);
    check.view(&mut t, Area::new(0, 0, 3, 10));
    let region = t.take_dirty_rect().expect("it painted");
    assert!(region.w > 3, "the indicator alone is a character wide");
}
