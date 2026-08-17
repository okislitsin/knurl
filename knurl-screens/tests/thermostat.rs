//! The custom widget, held to the same bar as a built-in one.
//!
//! Every clause of the contract in `knurl::custom_widget` is a test here: the
//! outcomes at the edges, a no-op event leaving the gate clean, the zero-area
//! guard, and the dirty behaviour that decides what a frame costs.
//!
//! Outside `src/` for the usual reason - a screen file may not mention the
//! recording target, which no device has.

use knurl::{Area, Component, Msg, Outcome, RenderTarget, Screen, Style};
use knurl_core::mock::{Op, RecordingTarget};
use knurl_screens::thermostat::{Thermostat, ThermostatScreen};

/// Two text rows, which is what the widget wants for value plus scale.
const AREA: Area = Area::new(2, 4, 100, 22);

fn painted(w: &Thermostat, area: Area) -> RecordingTarget {
    let mut t = RecordingTarget::new(128, 64);
    w.view(&mut t, area);
    t
}

fn drew(t: &RecordingTarget, needle: &str) -> bool {
    t.ops()
        .iter()
        .any(|op| matches!(op, Op::Text { text, .. } if text.contains(needle)))
}

// ── The dirty gate ───────────────────────────────────────────────────────────

#[test]
fn a_fresh_dial_paints_once_and_then_goes_quiet() {
    let dial = Thermostat::new(20);
    assert!(dial.dirty(), "a fresh widget owes its first paint");

    let mut t = RecordingTarget::new(128, 64);
    dial.view(&mut t, AREA);
    assert_eq!(
        t.ops().first(),
        Some(&Op::Clear { area: AREA }),
        "self-clear"
    );
    assert!(drew(&t, "20C"));
    assert_eq!(t.take_dirty_rect(), Some(AREA));

    dial.view(&mut t, AREA);
    assert_eq!(t.take_dirty_rect(), None, "nothing changed, nothing sent");
}

/// The clause every widget gets wrong first: an event that changes nothing must
/// leave the gate clean, or the widget repaints for the rest of the session.
#[test]
fn a_noop_update_leaves_it_clean() {
    let mut dial = Thermostat::new(20);
    dial.view(&mut RecordingTarget::new(128, 64), AREA);
    assert!(!dial.dirty());

    for msg in [Msg::Tick, Msg::Char('x'), Msg::Left, Msg::Right, Msg::Back] {
        assert_eq!(dial.update(&msg), Outcome::Ignored, "{msg:?}");
        assert!(!dial.dirty(), "{msg:?} dirtied a widget it did not change");
    }

    // ...including the press that lands on the value already applied.
    assert_eq!(dial.update(&Msg::Select), Outcome::Activated);
    assert!(!dial.dirty(), "applying the applied value moved no pixel");
}

#[test]
fn a_real_change_dirties_and_shows() {
    let mut dial = Thermostat::new(20);
    dial.view(&mut RecordingTarget::new(128, 64), AREA);

    assert_eq!(dial.update(&Msg::Up), Outcome::Consumed);
    assert_eq!(dial.setpoint(), 21);
    assert!(dial.dirty());
    assert!(drew(&painted(&dial, AREA), "21C"));
}

// ── Outcomes ─────────────────────────────────────────────────────────────────

/// The edges hand the event back, which is how the cursor gets out of a widget
/// at all - the container reads `Ignored` and gives the event to the next zone.
#[test]
fn the_limits_hand_the_event_back_without_dirtying() {
    let mut hot = Thermostat::new(Thermostat::MAX_C);
    hot.view(&mut RecordingTarget::new(128, 64), AREA);
    assert_eq!(hot.update(&Msg::Up), Outcome::Ignored, "past the top");
    assert_eq!(hot.setpoint(), Thermostat::MAX_C);
    assert!(!hot.dirty());
    assert_eq!(hot.update(&Msg::Down), Outcome::Consumed, "and back down");

    let mut cold = Thermostat::new(Thermostat::MIN_C);
    cold.view(&mut RecordingTarget::new(128, 64), AREA);
    assert_eq!(cold.update(&Msg::Down), Outcome::Ignored, "past the bottom");
    assert_eq!(cold.setpoint(), Thermostat::MIN_C);
    assert!(!cold.dirty());
}

/// `Activated` is about the event, never the pixels: applying a setpoint the
/// dial had already applied repaints nothing and is still the user's decision.
#[test]
fn applying_is_activated_whether_or_not_anything_moves() {
    let mut dial = Thermostat::new(20);
    assert_eq!(dial.update(&Msg::Select), Outcome::Activated);
    assert_eq!(dial.applied(), 20);

    let _ = dial.update(&Msg::Up);
    assert_eq!(dial.applied(), 20, "turning does not apply");
    assert_eq!(dial.update(&Msg::Select), Outcome::Activated);
    assert_eq!(dial.applied(), 21);
}

// ── Drawing ──────────────────────────────────────────────────────────────────

/// `view` guards an empty area; the widget guards a *useless* one. A screen
/// hands over whatever its layout produced, and four pixels is a valid answer.
#[test]
fn a_tiny_area_draws_nothing_and_never_panics() {
    for area in [
        Area::new(0, 0, 0, 20),
        Area::new(0, 0, 100, 0),
        Area::new(0, 0, 3, 20),
        Area::new(0, 0, 100, 4),
        Area::new(0, 0, 1, 1),
    ] {
        // A fresh dial each time: `view` marks a widget clean whether or not
        // `draw` found room to paint, so a reused one would go quiet and the
        // test would pass by drawing nothing at all.
        let t = painted(&Thermostat::new(20), area);
        assert!(
            t.ops().iter().all(|op| matches!(op, Op::Clear { .. })),
            "drew into {area:?}"
        );
    }
}

/// One text row is enough for the value; the scale waits for a second one.
#[test]
fn the_scale_appears_only_when_there_is_room_for_it() {
    let one_row = painted(&Thermostat::new(20), Area::new(0, 0, 100, 10));
    assert!(drew(&one_row, "20C"));
    assert!(
        !one_row.ops().iter().any(|op| matches!(op, Op::Line { .. })),
        "the mark needs the row the screen did not give"
    );

    let two_rows = painted(&Thermostat::new(20), Area::new(0, 0, 100, 22));
    assert!(
        two_rows
            .ops()
            .iter()
            .any(|op| matches!(op, Op::Line { .. }))
    );
}

/// Semantic styles, not colours: the warm end of the dial says `Danger`, and a
/// theme (or a monochrome panel) decides what that looks like. Nothing here
/// mentions red, which is why the same widget reads on a 1-bit OLED.
#[test]
fn the_warm_end_is_a_style_and_not_a_colour() {
    let warns = |setpoint: i16| {
        painted(&Thermostat::new(setpoint), AREA)
            .ops()
            .iter()
            .any(|op| {
                matches!(
                    op,
                    Op::Text {
                        style: Style::Danger,
                        ..
                    } | Op::Fill {
                        style: Style::Danger,
                        ..
                    }
                )
            })
    };
    assert!(!warns(20), "a comfortable setpoint is just a value");
    assert!(warns(28), "a hot one is drawn as a warning");
}

/// Focus is the same full-width band the built-in widgets draw, and taking or
/// losing it is a repaint - otherwise the cursor would be invisible.
#[test]
fn focus_is_a_band_and_a_repaint() {
    let mut dial = Thermostat::new(20);
    dial.view(&mut RecordingTarget::new(128, 64), AREA);
    assert!(!dial.dirty());

    dial.focus();
    assert!(dial.dirty());
    let t = painted(&dial, AREA);
    assert!(
        t.ops().iter().any(|op| matches!(
            op,
            Op::Band {
                style: Style::Focus,
                ..
            }
        )),
        "no focus band: {:?}",
        t.ops()
    );

    dial.blur();
    assert!(dial.dirty(), "losing the cursor is a repaint too");
}

// ── On a screen ──────────────────────────────────────────────────────────────

/// The payoff of getting `Ignored` right: nothing on the screen implements
/// "leave the dial", the chain does it when the widget stops taking events.
#[test]
fn the_cursor_leaves_the_dial_at_its_limit_and_finds_the_way_out() {
    let mut screen = ThermostatScreen::new();
    screen.enter();
    assert_eq!(screen.state().focus_index(), 0, "the dial takes the cursor");

    // Down to the bottom of the scale, then one more.
    for _ in 0..(20 - Thermostat::MIN_C) {
        assert_eq!(screen.update(&Msg::Down), None);
        assert_eq!(screen.state().focus_index(), 0);
    }
    assert_eq!(screen.update(&Msg::Down), None);
    assert_eq!(screen.state().focus_index(), 1, "onto < Back");
    assert!(
        screen.update(&Msg::Select).is_some(),
        "and it is the way out"
    );
}

/// Turning the dial costs the dial, not the screen.
#[test]
fn turning_the_dial_repaints_the_dial_alone() {
    let area = Area::new(0, 0, 128, 54);
    let mut screen = ThermostatScreen::new();
    screen.enter();
    let mut t = RecordingTarget::new(128, 64);
    screen.view(&mut t, area);
    let _ = t.take_dirty_rect();
    assert_eq!(t.take_dirty_rect(), None, "settled");

    screen.update(&Msg::Up);
    screen.view(&mut t, area);
    let region = t.take_dirty_rect().expect("the setpoint moved");
    assert_eq!(region.h, 22, "the dial's two rows and nothing else");
}
