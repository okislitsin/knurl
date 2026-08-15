//! OLED demo - the [`knurl_screens`] application on a small monochrome
//! SSD1306-class panel.
//!
//! **This file is the host, not the application.** Everything the user sees and
//! drives - every screen, the navigation between them, the catalogue data -
//! lives in `knurl-screens`, a `no_std` crate that CI builds for
//! `thumbv6m-none-eabi`. What is left here is what a device would provide
//! instead: a panel, a keymap, a frame loop and the chrome around the body (on
//! this panel, one title row).
//!
//! The loop is the **dirty-gated partial redraw** ([`Simulator::run_gated`]):
//! idle frames are skipped, and a painted frame draws only what changed - an
//! animating indicator repaints its own rows, a screen transition repaints the
//! screen. `Tick` never goes through the screen's focus chain (which only feeds
//! the focused zone); it goes to [`App::tick`], which is where animation lives.
//!
//! Encoder model only: Up / Down / Select. ASCII text only (the mono font is
//! ASCII; non-ASCII renders blank).
//!
//! ```sh
//! cargo run -p knurl-sim --example oled              # 128x64 (default)
//! cargo run -p knurl-sim --example oled -- 128x128   # 128x128
//! ```

use knurl::{
    Align, Area, Component,
    Constraint::{Fill, Length},
    Msg, Title, VStack,
};
use knurl_screens::{App, Panel};
use knurl_sim::{Frame, SimConfig, Simulator};

/// The Pager screen's lines. On a device this is a `const` array exactly like
/// this one; the TFT demo hands the same screen a live log instead.
const LOG: &[&str] = &[
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

    let mut app = App::new(Panel::SMALL, LOG);
    let mut title = Title::new(app.title()).with_align(Align::Center);
    let mut shown = app.page();
    let mut repaint = true;

    sim.run_gated(move |target, msgs| {
        for msg in msgs {
            // Animation runs past the focus chain, so it is its own call.
            if let Msg::Tick = msg {
                repaint |= app.tick();
                continue;
            }
            repaint = true;
            app.update(msg);
        }
        if app.quit() {
            return Frame::Quit;
        }
        if !core::mem::take(&mut repaint) {
            return Frame::Skipped;
        }

        // Chrome: the title is the host's, so the host keeps it in step.
        if app.page() != shown {
            shown = app.page();
            title.set_text(app.title());
        }

        let (w, h) = (target.width(), target.height());
        let lh = target.line_height().max(1);
        if w == 0 || h < lh {
            return Frame::Skipped;
        }
        let [head, body] = VStack::split(Area::new(0, 0, w, h), &[Length(lh), Fill(1)]);
        title.view(target, head);
        app.view(
            target,
            Area::new(1, body.y, body.w.saturating_sub(1), body.h),
        );
        Frame::Painted
    });
}
