//! Smoke test - per-component partial redraw on a mono panel.
//!
//! There is **no global screen clear**. Each widget self-gates inside its own
//! `view`: a clean widget draws nothing (its pixels persist), a dirty one clears
//! only its own area and redraws. So:
//!
//! - **Idle ticks**: only the `Spinner` is dirty → only the spinner's little area
//!   repaints. The static `Title` and the `List` are untouched (no flicker, and
//!   on a real TFT only a few bytes go over SPI instead of the whole frame).
//! - **Encoder** (Up/Down): only the `List` repaints its own area.
//! - **Select**: opens a modal `Dialog` (it self-clears its box over the
//!   content). Up/Down move the buttons (only the dialog repaints); Select
//!   confirms and **closes** - a structural transition, so we force a full
//!   redraw (clear + `mark_dirty` every widget) to erase the modal cleanly.
//!
//! The console prints `frames`/`paints`; paints stay far below the ~60fps frame
//! rate. ASCII text only. Close the window to quit.
//!
//! Run with: `cargo run -p knurl-sim --example dirty`

use knurl_sim::core::{
    Area, Component,
    Constraint::{Fill, Length},
    Dialog, List, Msg, Spinner, Title, VStack,
};
use knurl_sim::{Frame, SimConfig, Simulator};

const ITEMS: &[&str] = &[
    "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota", "Kappa",
];
const BTNS: &[&str] = &["OK", "Cancel"];

fn main() {
    let mut sim = Simulator::new(SimConfig {
        title: "knurl-sim - partial redraw (Up/Dn, Sel=modal, close to quit)".to_string(),
        ..Default::default()
    });

    let mut list = List::new(ITEMS);
    let mut spinner = Spinner::new().with_label("Working");
    let title = Title::new("Partial Redraw");
    let mut dialog = Dialog::new("Confirm", "Apply?", BTNS);
    let mut dialog_open = false;
    let mut closing = false; // set when the modal just closed → force full redraw

    let mut frames = 0u32;
    let mut paints = 0u32;

    sim.run_gated(move |target, msgs| {
        frames += 1;
        for msg in msgs {
            if dialog_open {
                dialog.update(msg);
                if dialog.is_confirmed() {
                    dialog.reset();
                    dialog_open = false;
                    closing = true;
                }
            } else if matches!(msg, Msg::Select) {
                dialog_open = true;
                dialog.mark_dirty(); // ensure the modal paints when it opens
            } else {
                list.update(msg);
                spinner.update(msg);
            }
        }

        let w = target.width();
        let h = target.height();
        let lh = target.line_height();

        // Structural transition (modal closed): clear the whole screen and mark
        // every widget dirty so the revealed content repaints with no leftovers.
        if closing {
            target.clear(Area::new(0, 0, w, h));
            title.mark_dirty();
            spinner.mark_dirty();
            list.mark_dirty();
            closing = false;
        }

        // Frame gate: skip entirely when nothing is dirty.
        let dirty =
            title.dirty() || spinner.dirty() || list.dirty() || (dialog_open && dialog.dirty());
        if !dirty {
            return Frame::Skipped;
        }

        // Paint: each widget self-gates and clears only its own area.
        let [head, spin, body] = VStack::split(
            Area::new(0, 0, w, h),
            &[Length(lh + 2), Length(lh + 2), Fill(1)],
        );
        title.view(target, Area::new(head.x + 2, head.y, head.w, head.h));
        spinner.view(target, Area::new(spin.x + 2, spin.y, spin.w, spin.h));
        list.view(
            target,
            Area::new(body.x + 2, body.y, body.w.saturating_sub(4), body.h),
        );
        if dialog_open {
            let dw = (w * 3) / 4;
            let dh = (lh * 4).min(h);
            dialog.view(target, Area::new((w - dw) / 2, (h - dh) / 2, dw, dh));
        }

        paints += 1;
        if paints.is_multiple_of(20) {
            println!(
                "frames={frames} paints={paints} (skipped {})",
                frames - paints
            );
        }
        Frame::Painted
    });
}
