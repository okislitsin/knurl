//! Every screen of the demo, driven headlessly.
//!
//! The demo deliberately changed shape in this step (the Layout and Tabs pages
//! became compositions instead of showcases), so a golden trace would only be
//! able to say "it differs". What is worth checking instead: that every page in
//! the catalogue still opens and paints, that a way out exists from each of
//! them, and that the new compositions behave - the cursor walking between two
//! forms, and a tab switch actually repainting the form that arrives.
//!
//! These tests live outside `src/` on purpose: a screen file must not mention
//! anything a device would not have, and the recording target is a test-only
//! facility of `knurl-core`.

use knurl::{
    Area, Button, FocusChain, FocusZone, Form, FormField, Msg, Outcome, Picker, RenderTarget,
    Screen, ScreenState, Slider,
};
use knurl_core::mock::{Op, RecordingTarget};
use knurl_screens::{
    App, AppEvent, MENU, Page, Panel, canvas::CanvasScreen, menu::MenuScreen,
    tab_forms::TabFormsScreen, text::TextScreen, two_forms::TwoFormsScreen,
};

/// What a screen gets on a 128x64 panel once the title row is taken.
const BODY: Area = Area::new(0, 0, 128, 54);
const LOG: &[&str] = &["[001] boot", "[002] ready", "[003] idle"];

fn app() -> App<'static, [&'static str]> {
    App::new(Panel::SMALL, LOG)
}

fn painted(app: &mut App<'static, [&'static str]>) -> RecordingTarget {
    let mut t = RecordingTarget::new(128, 64);
    app.view(&mut t, BODY);
    t
}

fn drew(t: &RecordingTarget, needle: &str) -> bool {
    t.ops()
        .iter()
        .any(|op| matches!(op, Op::Text { text, .. } if text.contains(needle)))
}

/// The menu row carrying `label` - so a test names the page it wants and a new
/// catalogue entry does not renumber every test below.
fn row_of(label: &str) -> usize {
    MENU.iter()
        .position(|m| *m == label)
        .unwrap_or_else(|| panic!("no menu row {label:?}"))
}

/// Opens the menu row `row` on a fresh application.
fn open(row: usize) -> App<'static, [&'static str]> {
    let mut app = app();
    for _ in 0..row {
        app.update(&Msg::Down);
    }
    app.update(&Msg::Select);
    app
}

// ── The catalogue ────────────────────────────────────────────────────────────

/// Every row of the menu opens its page, and every page draws something. This
/// is the sweep that says no widget was lost in the rework.
#[test]
fn every_menu_row_opens_a_page_that_paints() {
    for (row, label) in MENU.iter().enumerate().take(MENU.len() - 1) {
        let mut app = open(row);
        let expected = Page::from_menu_row(row).expect("a page for every row but Exit");
        assert_eq!(app.page(), expected, "row {row} ({label})");

        let t = painted(&mut app);
        assert!(
            !t.ops().is_empty(),
            "page {} painted nothing at all",
            expected.title()
        );
        // Every page but the root has a way out, and it is drawn.
        assert!(
            drew(&t, "< Back"),
            "page {} has no way out",
            expected.title()
        );
    }
}

/// ...and rotating to the end of any page lands on that way out, so one press
/// returns to the menu. Nothing in a screen implements this: the chain hands
/// the cursor on when a zone runs out.
#[test]
fn every_page_can_be_left_by_rotating_to_the_end_and_pressing() {
    for row in 0..MENU.len() - 1 {
        let mut app = open(row);
        let page = app.page();
        // Long enough for the longest list in the catalogue.
        for _ in 0..40 {
            app.update(&Msg::Down);
        }
        app.update(&Msg::Select);
        assert_eq!(app.page(), Page::Menu, "{} could not be left", page.title());
        assert!(!app.quit(), "leaving a page is not leaving the demo");
    }
}

/// The last menu row is the way out of the demo itself.
#[test]
fn the_last_menu_row_quits() {
    let app = open(MENU.len() - 1);
    assert!(app.quit());
}

/// The menu reports the page its cursor is on, not the row number of anything
/// else - checked against the table the rows are built from.
#[test]
fn the_menu_reports_the_page_under_its_cursor() {
    let mut menu = MenuScreen::new();
    menu.enter();
    for row in 0..MENU.len() - 1 {
        if row > 0 {
            assert_eq!(menu.update(&Msg::Down), None, "a step is not an event");
        }
        assert_eq!(
            menu.update(&Msg::Select),
            Some(AppEvent::Open(Page::from_menu_row(row).unwrap())),
            "row {row}"
        );
    }
    assert_eq!(menu.update(&Msg::Down), None);
    assert_eq!(menu.update(&Msg::Select), Some(AppEvent::Quit));
}

// ── The screen that draws itself ─────────────────────────────────────────────

/// The canvas screen's two ways of running a canvas, as behaviour: the gauge
/// and the sparkline are built per frame and always paint; the icon is a field,
/// so it paints once, goes quiet, and comes back only because `on_enter` marks
/// it dirty - the trap every stored canvas has, since the screen's repaint
/// cascade walks zones and a canvas is not one.
#[test]
fn the_canvas_screen_paints_live_drawings_and_a_gated_sprite() {
    let count = |t: &RecordingTarget| {
        let lines = t
            .ops()
            .iter()
            .filter(|op| matches!(op, Op::Line { .. }))
            .count();
        let sprites = t
            .ops()
            .iter()
            .filter(|op| matches!(op, Op::Bitmap { .. }))
            .count();
        (lines, sprites)
    };

    let mut screen = CanvasScreen::new();
    screen.enter();

    let mut first = RecordingTarget::new(128, 64);
    screen.view(&mut first, BODY);
    let (lines, sprites) = count(&first);
    assert!(lines > 10, "the gauge and the sparkline: {lines} lines");
    assert_eq!(sprites, 1, "the icon paints on the first frame");

    let mut idle = RecordingTarget::new(128, 64);
    screen.view(&mut idle, BODY);
    let (lines, sprites) = count(&idle);
    assert!(lines > 10, "a canvas built per frame always paints");
    assert_eq!(sprites, 0, "…and one kept as a field has gone quiet");

    // Leaving and coming back clears the screen, so the sprite has to be told.
    screen.enter();
    let mut again = RecordingTarget::new(128, 64);
    screen.view(&mut again, BODY);
    assert_eq!(count(&again).1, 1, "the icon never came back");
}

/// It animates past the focus chain, like every other live screen.
#[test]
fn the_canvas_screen_ticks() {
    let mut screen = CanvasScreen::new();
    screen.enter();
    assert!(screen.tick(), "a live drawing is worth a frame");
}

// ── Two forms in a layout ────────────────────────────────────────────────────

/// The headline composition: one cursor, two forms, and the step between them
/// is the chain's, not the screen's.
#[test]
fn the_cursor_walks_out_of_one_form_and_into_the_other() {
    let mut screen = TwoFormsScreen::new(Panel::SMALL);
    screen.enter();
    assert_eq!(screen.state().focus_index(), 0);

    // Down through the first form's two fields, then off its end.
    assert_eq!(screen.update(&Msg::Down), None, "inside the first form");
    assert_eq!(screen.state().focus_index(), 0);
    assert_eq!(screen.update(&Msg::Down), None);
    assert_eq!(screen.state().focus_index(), 1, "into the second form");

    // ...and back up again, which must land on the first form's *last* field -
    // otherwise the next Up would throw the cursor straight out of it.
    assert_eq!(screen.update(&Msg::Up), None);
    assert_eq!(screen.state().focus_index(), 0);
    assert_eq!(screen.update(&Msg::Up), None, "still inside the first form");
    assert_eq!(screen.state().focus_index(), 0);

    // Past the second form is the way out.
    for _ in 0..4 {
        screen.update(&Msg::Down);
    }
    assert_eq!(screen.state().focus_index(), 2);
    assert_eq!(screen.update(&Msg::Select), Some(AppEvent::GoBack));
}

/// Both forms are painted, on either layout - the screen splits sideways when
/// it has the width and downwards when it does not.
#[test]
fn both_forms_are_painted_in_either_layout() {
    for area in [Area::new(0, 0, 128, 54), Area::new(0, 0, 312, 200)] {
        let mut screen = TwoFormsScreen::new(Panel::SMALL);
        screen.enter();
        let mut t = RecordingTarget::new(area.w, area.h);
        screen.view(&mut t, area);
        assert!(drew(&t, "Fan"), "first form missing at {}px", area.w);
        assert!(drew(&t, "Lamp"), "second form missing at {}px", area.w);
    }
}

// ── Three forms behind tabs ──────────────────────────────────────────────────

/// The hole Step 2 left open, now a live case: the form that arrives on a tab
/// switch has been off-screen and is clean, so unless the switch is reported
/// the screen keeps showing the previous tab's fields.
#[test]
fn switching_tabs_repaints_the_form_that_arrives() {
    let mut screen = TabFormsScreen::new(Panel::SMALL);
    screen.enter();

    let mut first = RecordingTarget::new(128, 64);
    screen.view(&mut first, BODY);
    assert!(drew(&first, "On"), "the first tab's form");
    assert!(!screen.state().dirty(), "and the screen has settled");

    // Rotate onto the second tab.
    assert_eq!(screen.update(&Msg::Down), None);
    assert!(
        screen.state().dirty(),
        "the switch did not invalidate the screen"
    );

    let mut second = RecordingTarget::new(128, 64);
    screen.view(&mut second, BODY);
    assert!(
        second.ops().contains(&Op::Clear { area: BODY }),
        "the outgoing form's rows were never wiped"
    );
    assert!(drew(&second, "DHCP"), "the incoming form was not painted");
}

/// The tab area behaves like any other zone: `Select` drops into the page,
/// rotating off its top climbs back onto the strip, off its bottom leaves for
/// the way out underneath.
#[test]
fn the_cursor_enters_a_tabs_form_and_leaves_it_again() {
    let mut screen = TabFormsScreen::new(Panel::SMALL);
    screen.enter();

    assert_eq!(screen.update(&Msg::Select), None, "into the tab's form");
    assert_eq!(screen.state().focus_index(), 0, "still the tab zone");
    // Down through the form's two fields, then out of the tab area entirely.
    for _ in 0..2 {
        assert_eq!(screen.update(&Msg::Down), None);
    }
    assert_eq!(screen.state().focus_index(), 1, "onto < Back");
    assert_eq!(screen.update(&Msg::Select), Some(AppEvent::GoBack));
}

// ── What a frame costs on the bus ────────────────────────────────────────────

/// The demo on a 320x240 colour panel, laid out exactly as `examples/tft.rs`
/// does it: a title row, the application's body, a status-bar hint. The
/// recording target's metrics are `FONT_6X10`'s, so these are the same pixels
/// the simulator draws.
struct Tft {
    app: App<'static, [&'static str]>,
    target: RecordingTarget,
}

impl Tft {
    /// The body area the host hands the application (`tft.rs`, verbatim).
    const BODY: Area = Area::new(4, 12, 312, 216);

    fn open(row: usize) -> Self {
        let mut tft = Self {
            app: app(),
            target: RecordingTarget::new(320, 240),
        };
        for _ in 0..row {
            tft.app.update(&Msg::Down);
        }
        tft.app.update(&Msg::Select);
        tft.frame(); // the arriving screen repaints over its predecessor
        tft.frame(); // ...and settles
        tft
    }

    /// Paints one frame and returns the region it would cost to push.
    fn frame(&mut self) -> Option<Area> {
        self.app.view(&mut self.target, Self::BODY);
        self.target.take_dirty_rect()
    }

    fn send(&mut self, msg: &Msg) {
        self.app.update(msg);
    }
}

/// A settled screen with nothing dirty draws nothing at all - so the frame
/// costs no bytes, rather than a panel's worth of them.
#[test]
fn an_idle_frame_has_no_region() {
    let mut tft = Tft::open(row_of("List"));
    assert_eq!(tft.frame(), None);
    assert_eq!(tft.frame(), None);
}

/// Rotating the encoder in a list repaints **the list**, not the panel: the
/// granularity is the widget, because that is what clears its own area.
#[test]
fn rotating_in_a_list_repaints_the_list_and_not_the_panel() {
    let mut tft = Tft::open(row_of("List"));
    tft.send(&Msg::Down);
    let region = tft.frame().expect("the cursor moved");
    // The screen's list: the body minus the "< Back" row under it.
    assert_eq!(region, Area::new(4, 12, 312, 206));
    assert!(
        region.h < Tft::BODY.h,
        "the way out below the list was not touched"
    );
}

/// Editing a value costs its row. This is the interaction the encoder spends
/// its life on, and 10 rows of a 240px panel is what it should cost.
#[test]
fn a_value_being_edited_costs_one_row() {
    let mut tft = Tft::open(row_of("Editors"));
    tft.send(&Msg::Down);
    tft.send(&Msg::Down);
    tft.send(&Msg::Select); // into edit
    let _ = tft.frame();
    tft.send(&Msg::Up); // one step
    let region = tft.frame().expect("the value changed");
    assert_eq!(region.h, 10, "one text row: {region:?}");
}

/// Moving inside a form behind tabs leaves the strip alone - two rows, and the
/// top of the panel is not in them. Before the strip had a dirty gate it was
/// repainted on every event, and the region reached up to it.
#[test]
fn moving_inside_a_tabs_form_leaves_the_strip_alone() {
    let mut tft = Tft::open(row_of("Tabs + forms"));
    tft.send(&Msg::Select); // into the active tab's form
    let _ = tft.frame();
    tft.send(&Msg::Down); // between the form's fields
    let region = tft.frame().expect("the focus moved");
    let strip = Area::new(Tft::BODY.x, Tft::BODY.y, Tft::BODY.w, 10);
    assert_eq!(region, Area::new(4, 22, 312, 20), "the two rows involved");
    assert!(
        region.intersect(strip).is_none(),
        "the tab strip is not in {region:?}"
    );
}

/// The anti-pattern the library documents, measured on the demo that used to
/// have it. Every row of the Indicators screen lives in a field, so a tick
/// costs the indicators that moved - one text row when only the spinner
/// advances - instead of the whole hand-drawn window.
#[test]
fn a_tick_costs_the_indicators_that_moved_and_not_the_window() {
    let mut tft = Tft::open(row_of("Indicators"));
    assert_eq!(tft.frame(), None, "a settled screen draws nothing");

    // The first tick advances the spinner alone: the triangle wave only moves
    // every second tick, and a setter that lands on the value it already had
    // does not dirty.
    tft.app.tick();
    let spinner_only = tft.frame().expect("the spinner advanced");
    assert_eq!(
        spinner_only,
        Area::new(4, 12, 312, 10),
        "one text row: the spinner's"
    );

    // The next one moves the bar and the gauge as well. They sit two and four
    // rows below the spinner, and the region is one box over all three - which
    // is the documented trade in `DirtyRect`, not a widget repainting too much.
    tft.app.tick();
    let all_three = tft.frame().expect("the level moved");
    assert_eq!(all_three, Area::new(4, 12, 312, 50), "rows 0 through 4");
    assert!(
        all_three.h < Tft::BODY.h / 2,
        "still a fraction of the panel: {all_three:?}"
    );
}

/// A screen of static rows costs nothing at all once it has settled - it used
/// to report the whole window on every frame, because it rebuilt its rows
/// inside `draw` and cleared the window to paint them.
#[test]
fn a_hand_drawn_stack_of_static_rows_settles_to_no_region_at_all() {
    let mut tft = Tft::open(row_of("Text"));
    assert_eq!(tft.frame(), None);
    assert_eq!(tft.frame(), None);

    // Scrolling is what the window is for, and it costs the window - once. A
    // 320x240 panel fits all nine rows, so this needs the small one, where the
    // same screen has something to scroll.
    let window = Area::new(0, 0, 128, 44); // 4 rows, plus the "< Back" row
    let mut screen = TextScreen::new();
    screen.enter();
    let mut t = RecordingTarget::new(128, 64);
    screen.view(&mut t, window);
    let _ = t.take_dirty_rect();
    assert_eq!(t.take_dirty_rect(), None);

    assert_eq!(screen.update(&Msg::Down), None, "the window scrolled");
    screen.view(&mut t, window);
    assert_eq!(
        t.take_dirty_rect(),
        Some(Area::new(0, 0, 128, 34)),
        "the window, and not the row of chrome under it"
    );
    screen.view(&mut t, window);
    assert_eq!(t.take_dirty_rect(), None, "and it goes quiet again");
}

/// Opening another screen is a full repaint, and should be: everything on the
/// panel belongs to the screen that just left.
#[test]
fn opening_a_screen_repaints_all_of_it() {
    let mut app = app();
    app.update(&Msg::Down); // onto "List"
    let mut target = RecordingTarget::new(320, 240);
    app.view(&mut target, Tft::BODY);
    let _ = target.take_dirty_rect();

    app.update(&Msg::Select);
    app.view(&mut target, Tft::BODY);
    assert_eq!(target.take_dirty_rect(), Some(Tft::BODY));
}

// ── Scenarios: the compositions, driven end to end ───────────────────────────

/// The router, the screens and the chain as one thing: open every page in turn
/// from a single application, leave each one, and arrive back at the menu row
/// that opened it. A per-screen test cannot see this - it is the handover that
/// breaks, not either side of it.
#[test]
fn one_application_opens_every_page_in_turn_and_comes_back_each_time() {
    let mut app = app();
    for (row, label) in MENU.iter().enumerate().take(MENU.len() - 1) {
        // The menu is where the last visit left it, so this is a relative step.
        if row > 0 {
            app.update(&Msg::Down);
        }
        app.update(&Msg::Select);
        assert_eq!(
            app.page(),
            Page::from_menu_row(row).unwrap(),
            "row {row} ({label}) opened the wrong page"
        );

        let mut t = RecordingTarget::new(128, 64);
        app.view(&mut t, BODY);
        assert!(drew(&t, "< Back"), "{label} has no way out");

        for _ in 0..40 {
            app.update(&Msg::Down);
        }
        app.update(&Msg::Select);
        assert_eq!(app.page(), Page::Menu, "{label} could not be left");
        assert!(!app.quit(), "leaving a page is not leaving the demo");
    }
    // ...and the menu cursor is on the last page visited, not back at the top.
    app.update(&Msg::Down);
    app.update(&Msg::Select);
    assert!(app.quit(), "the row after the last page is Exit");
}

/// The y of the list cursor's marker - the only thing on the panel that says
/// where the selection is.
fn marker_row(t: &RecordingTarget) -> Option<u16> {
    t.ops().iter().find_map(|op| match op {
        Op::Text { text, y, .. } if text.trim() == ">" => Some(*y),
        _ => None,
    })
}

/// A screen keeps its state while it is away. The router holds the screen, so
/// coming back finds the cursor where it was left - which is what makes "Back"
/// cheap on a device and why the demo does not rebuild a screen per visit.
#[test]
fn a_screen_returned_to_is_where_it_was_left() {
    let mut app = app();
    let list = row_of("List");
    for _ in 0..list {
        app.update(&Msg::Down);
    }
    app.update(&Msg::Select);
    // A frame first: a scrolling widget learns its window size from `view`, so
    // input that arrives before the first paint moves the cursor without
    // scrolling under it. On a device the frame always comes first.
    app.view(&mut RecordingTarget::new(128, 64), BODY);

    // Rotate to the end of the list, which is also how the cursor reaches the
    // way out - so this is the state the screen is actually left in.
    for _ in 0..40 {
        app.update(&Msg::Down);
    }
    let mut before = RecordingTarget::new(128, 64);
    app.view(&mut before, BODY);
    let was = marker_row(&before).expect("the list drew its cursor");

    app.update(&Msg::Select);
    assert_eq!(app.page(), Page::Menu);
    app.update(&Msg::Select);
    assert_eq!(app.page(), Page::List);

    let mut after = RecordingTarget::new(128, 64);
    app.view(&mut after, BODY);
    assert_eq!(
        marker_row(&after),
        Some(was),
        "the list forgot where its cursor was"
    );
}

/// Tabs with live forms, all the way round: into the page, edit a value there,
/// out to the strip, over to the other tab and back - and the value is still
/// what it was set to, and painted.
#[test]
fn a_value_edited_behind_a_tab_survives_the_trip_to_another_tab() {
    let mut screen = TabFormsScreen::new(Panel::SMALL);
    screen.enter();

    // Into the first tab's form, onto its second field, into the edit.
    assert_eq!(screen.update(&Msg::Select), None, "into the page");
    assert_eq!(screen.update(&Msg::Down), None, "onto the next field");
    assert_eq!(screen.update(&Msg::Select), None, "into the edit");
    assert_eq!(screen.update(&Msg::Up), None, "one step of the value");
    assert_eq!(screen.update(&Msg::Select), None, "out of the edit");

    let mut edited = RecordingTarget::new(128, 64);
    screen.view(&mut edited, BODY);
    let value = value_row(&edited).expect("the edited row was painted");

    // Back onto the strip, across to the other tab, and back again.
    assert_eq!(screen.update(&Msg::Up), None);
    assert_eq!(screen.update(&Msg::Up), None, "onto the strip");
    assert_eq!(screen.update(&Msg::Down), None, "the second tab");
    let mut other = RecordingTarget::new(128, 64);
    screen.view(&mut other, BODY);
    assert!(drew(&other, "DHCP"), "the second tab's form is not showing");

    assert_eq!(screen.update(&Msg::Up), None, "back to the first tab");
    let mut again = RecordingTarget::new(128, 64);
    screen.view(&mut again, BODY);
    assert!(
        again.ops().contains(&Op::Clear { area: BODY }),
        "the outgoing form's rows were never wiped"
    );
    assert_eq!(
        value_row(&again),
        Some(value),
        "the value did not survive the trip"
    );
}

/// The right-hand value of the second row of a form - what a `Counter` or a
/// `Picker` shows, and the thing an edit is supposed to have changed.
fn value_row(t: &RecordingTarget) -> Option<String> {
    let lh = 10; // the recording target's font
    t.ops()
        .iter()
        .filter_map(|op| match op {
            Op::Text { text, y, x, .. } if *y == lh && *x > 40 => Some(text.clone()),
            _ => None,
        })
        .next_back()
}

// ── A form that changes shape under the cursor ───────────────────────────────

/// The Step 1 scenario, whole: a `Picker` that grows three sliders under itself
/// in one mode and takes them away in another. The form holds no fields - the
/// slice is passed per call - so the screen simply builds a different array,
/// and everything else (focus, edit mode, the re-layout repaint) has to follow.
struct ModeScreen {
    state: ScreenState,
    form: Form,
    mode: Picker<'static>,
    red: Slider<'static>,
    green: Slider<'static>,
    blue: Slider<'static>,
    back: Button<'static>,
}

const MODES: &[&str] = &["Off", "RGB"];

impl ModeScreen {
    fn new() -> Self {
        Self {
            state: ScreenState::new(),
            form: Form::new(),
            mode: Picker::new("Mode", MODES),
            red: Slider::new("R").with_range(0, 9),
            green: Slider::new("G").with_range(0, 9),
            blue: Slider::new("B").with_range(0, 9),
            back: Button::new("< Back"),
        }
    }

    fn rgb(&self) -> bool {
        self.mode.selected() == 1
    }
}

impl Screen for ModeScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let rgb = self.rgb();
        let Self {
            state,
            form,
            mode,
            red,
            green,
            blue,
            back,
        } = self;
        if rgb {
            let mut fields: [&mut dyn FormField; 5] = [mode, red, green, blue, back];
            let mut zone = form.zone(&mut fields);
            f(state.chain(), &mut [&mut zone]);
        } else {
            let mut fields: [&mut dyn FormField; 2] = [mode, back];
            let mut zone = form.zone(&mut fields);
            f(state.chain(), &mut [&mut zone]);
        }
    }

    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let rgb = self.rgb();
        let Self {
            form,
            mode,
            red,
            green,
            blue,
            back,
            ..
        } = self;
        if rgb {
            let fields: [&mut dyn FormField; 5] = [mode, red, green, blue, back];
            form.view(target, area, &fields);
        } else {
            let fields: [&mut dyn FormField; 2] = [mode, back];
            form.view(target, area, &fields);
        }
    }
}

#[test]
fn a_form_that_grows_under_the_cursor_repaints_and_keeps_its_focus() {
    let area = Area::new(0, 0, 128, 54);
    let mut screen = ModeScreen::new();
    screen.enter();
    let mut t = RecordingTarget::new(128, 64);
    screen.view(&mut t, area);
    assert!(!drew(&t, "R"), "no sliders in the Off mode");

    // Into the picker's edit, one step to RGB, out again.
    assert_eq!(screen.update(&Msg::Select), None, "into the edit");
    assert_eq!(screen.update(&Msg::Down), None, "Off -> RGB");
    assert_eq!(screen.update(&Msg::Select), None, "out of the edit");
    assert_eq!(
        screen.form.focus_index(),
        0,
        "the cursor stayed on the mode"
    );

    // The stack is a different shape now, so the form repaints all of it - and
    // the sliders that arrived are on the panel.
    let mut grown = RecordingTarget::new(128, 64);
    screen.view(&mut grown, area);
    assert!(
        grown.ops().contains(&Op::Clear { area }),
        "a re-layout has to wipe the stack it is replacing"
    );
    for label in ["R", "G", "B"] {
        assert!(drew(&grown, label), "the {label} slider never arrived");
    }

    // The cursor walks into the new fields, which is the point of them.
    assert_eq!(screen.update(&Msg::Down), None);
    assert_eq!(screen.form.focus_index(), 1, "into the first slider");

    // ...and back to Off takes them away again, wiping what they left behind.
    assert_eq!(screen.update(&Msg::Up), None, "back onto the mode");
    assert_eq!(screen.update(&Msg::Select), None);
    assert_eq!(screen.update(&Msg::Up), None, "RGB -> Off");
    assert_eq!(screen.update(&Msg::Select), None);
    let mut shrunk = RecordingTarget::new(128, 64);
    screen.view(&mut shrunk, area);
    assert!(
        shrunk.ops().contains(&Op::Clear { area }),
        "the rows the sliders had are still on the panel"
    );
    assert!(!drew(&shrunk, "R"), "a slider outlived its mode");
    assert!(
        drew(&shrunk, "< Back"),
        "the way out came back up the stack"
    );
}
