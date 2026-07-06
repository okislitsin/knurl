//! Smoke test - per-component partial redraw on a colour TFT.
//!
//! The colour twin of `dirty`. No global clear: each widget self-gates inside
//! `view`, so an idle tick repaints only the `Spinner`'s area, encoder input
//! repaints only the `List`, and opening/closing the modal `Dialog` repaints
//! cleanly (close forces a full redraw to erase the modal). On a 320x240 panel a
//! full repaint is ~150KB over SPI, so repainting only the changed widget is the
//! whole point - the console `frames`/`paints` show the gate working.
//!
//! ASCII text only. Close the window to quit.
//!
//! Run with: `cargo run -p knurl-sim --example dirty_color`

use knurl_sim::core::{
    Area, Component,
    Constraint::{Fill, Length},
    Dialog, List, Msg, Spinner, Title, VStack,
};
use knurl_sim::{ColorSimConfig, ColorSimulator, Frame};

const ITEMS: &[&str] = &[
    "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota", "Kappa",
    "Lambda", "Mu", "Nu", "Xi", "Omicron", "Pi",
];
const BTNS: &[&str] = &["OK", "Cancel"];

fn main() {
    let mut sim = ColorSimulator::new(ColorSimConfig {
        title: "knurl-sim (color) - partial redraw (Up/Dn, Sel=modal)".to_string(),
        ..Default::default()
    });

    let mut list = List::new(ITEMS);
    let mut spinner = Spinner::new().with_label("Working");
    let title = Title::new("Partial Redraw");
    let mut dialog = Dialog::new("Confirm", "Apply?", BTNS);
    let mut dialog_open = false;
    let mut closing = false;

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
                dialog.mark_dirty();
            } else {
                list.update(msg);
                spinner.update(msg);
            }
        }

        let w = target.width();
        let h = target.height();
        let lh = target.line_height();

        if closing {
            target.clear(Area::new(0, 0, w, h));
            title.mark_dirty();
            spinner.mark_dirty();
            list.mark_dirty();
            closing = false;
        }

        let dirty = title.dirty()
            || spinner.dirty()
            || list.dirty()
            || (dialog_open && dialog.dirty());
        if !dirty {
            return Frame::Skipped;
        }

        let [head, spin, body] = VStack::split(
            Area::new(0, 0, w, h),
            &[Length(lh + 4), Length(lh + 4), Fill(1)],
        );
        title.view(target, Area::new(head.x + 4, head.y, head.w, head.h));
        spinner.view(target, Area::new(spin.x + 4, spin.y, spin.w, spin.h));
        list.view(target, Area::new(body.x + 4, body.y, body.w.saturating_sub(8), body.h));
        if dialog_open {
            let dw = (w * 3) / 4;
            let dh = (lh * 4).min(h);
            dialog.view(target, Area::new((w - dw) / 2, (h - dh) / 2, dw, dh));
        }

        paints += 1;
        if paints.is_multiple_of(20) {
            println!("frames={frames} paints={paints} (skipped {})", frames - paints);
        }
        Frame::Painted
    });
}
