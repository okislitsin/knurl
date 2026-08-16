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

use knurl::{Area, Msg, Screen};
use knurl_core::mock::{Op, RecordingTarget};
use knurl_screens::{
    App, AppEvent, MENU, Page, Panel, canvas::CanvasScreen, menu::MenuScreen,
    tab_forms::TabFormsScreen, two_forms::TwoFormsScreen,
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
