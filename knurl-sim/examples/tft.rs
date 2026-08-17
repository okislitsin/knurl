//! TFT demo - the same [`knurl_screens`] application on a 320x240 colour panel.
//!
//! It runs the **same screens** as `oled.rs`, from the same `no_std` crate, and
//! differs only in what a host provides: the panel and its theme, the chrome
//! (a title row and a status-bar hint), and - because a desktop can - a **live
//! log** behind the Pager screen.
//!
//! That log is the point of the [`LinesModel`] seam: `StreamLog` here is a
//! `std` ring buffer that grows while the user watches it, and the Pager screen
//! that renders it is the same file a device builds against a `const` array. No
//! screen knows which it got.
//!
//! Rendered through the shipped Charm [`ColorTheme`] - calm palette, no
//! hardcoded RGB - and the dirty-gated partial-redraw loop
//! ([`ColorSimulator::run_gated`]).
//!
//! Encoder model only: Up / Down / Select. ASCII text only.
//!
//! ```sh
//! cargo run -p knurl-sim --example tft
//! ```

use std::cell::{Cell, RefCell};

use knurl::{
    Align, Area, Component,
    Constraint::{Fill, Length},
    LinesModel, Msg, StatusBar, Title, VStack,
};
use knurl_screens::{App, Panel};
use knurl_sim::{ColorSimConfig, ColorSimulator, Frame};

// ── A live log (the host's, not the screen's) ─────────────────────────────────

/// A capped ring buffer of recent lines - a stand-in for a live UART. Interior
/// mutability so the frame loop can append (`&self`) while the Pager screen
/// borrows it, and `write_line` so it never hands out a borrow of its buffer.
struct StreamLog {
    lines: RefCell<Vec<String>>,
    cap: usize,
    /// Appends so far - the model's `revision`. A ring buffer's length is no
    /// use for that: it stops moving the moment the buffer is full, and lines
    /// keep arriving.
    writes: Cell<u32>,
}

impl StreamLog {
    fn new(cap: usize) -> Self {
        Self {
            lines: RefCell::new(Vec::new()),
            cap,
            writes: Cell::new(0),
        }
    }

    fn push(&self, line: String) {
        let mut v = self.lines.borrow_mut();
        v.push(line);
        if v.len() > self.cap {
            let excess = v.len() - self.cap;
            v.drain(0..excess);
        }
        self.writes.set(self.writes.get().wrapping_add(1));
    }
}

impl LinesModel for StreamLog {
    fn line_count(&self) -> usize {
        self.lines.borrow().len()
    }
    fn get_line(&self, _i: usize) -> &str {
        "" // unused: the Pager renders through write_line
    }
    fn write_line(&self, i: usize, out: &mut dyn core::fmt::Write) {
        if let Some(s) = self.lines.borrow().get(i) {
            let _ = out.write_str(s);
        }
    }
    /// What makes a new line appear without the screen being told. Follow mode
    /// only helps while there is something to scroll; a log that still fits
    /// leaves the pager's own state untouched, and the line would never be
    /// drawn.
    fn revision(&self) -> u32 {
        self.writes.get()
    }
}

fn main() {
    let mut sim = ColorSimulator::new(ColorSimConfig {
        title: "knurl TFT - 320x240 (Up/Down, Space)".to_string(),
        ..Default::default()
    });

    let log = StreamLog::new(256);
    let mut ticks = 0u32;
    let mut count = 0u32;
    for _ in 0..8 {
        count += 1;
        log.push(format!("[{count:03}] sensor = {}", (count * 37) % 1000));
    }

    let mut app = App::new(Panel::LARGE, &log).with_log_follow(true);
    let mut title = Title::new(app.title()).with_align(Align::Center);
    let mut shown = app.page();
    let mut repaint = true;
    let mut chrome = true; // the hint bar only changes with the screen

    // Non-move closure: `app` borrows `&log` and the body appends to it
    // (`&self`) - both shared borrows, so they coexist.
    sim.run_gated(|target, msgs| {
        for msg in msgs {
            if let Msg::Tick = msg {
                ticks += 1;
                if ticks.is_multiple_of(3) {
                    count += 1;
                    log.push(format!("[{count:03}] sensor = {}", (count * 37) % 1000));
                }
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

        if app.page() != shown {
            shown = app.page();
            title.set_text(app.title());
            chrome = true;
        }

        let (w, h) = (target.width(), target.height());
        let lh = target.line_height().max(1);
        if w == 0 || h < lh * 3 {
            return Frame::Skipped;
        }
        let [head, mid, foot] = VStack::split(
            Area::new(0, 0, w, h),
            &[Length(lh + 2), Fill(1), Length(lh + 2)],
        );
        title.view(target, Area::new(head.x, head.y + 1, head.w, lh));
        // The hint is chrome: repainting it every frame would undo the gate.
        if core::mem::take(&mut chrome) {
            StatusBar::new()
                .with_left(app.hint())
                .with_right("knurl")
                .view(target, foot);
        }
        app.view(target, Area::new(4, mid.y, mid.w.saturating_sub(8), mid.h));
        Frame::Painted
    });
}
