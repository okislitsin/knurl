//! A bare window - the minimal simulator smoke test.
//!
//! Opens an empty SDL2 window. Close the window to quit. (The default keymap is
//! the encoder model - ↑/↓ and Space - and this empty screen ignores all of it.)
//!
//! Run with: `cargo run -p knurl-sim --example empty`

use knurl_sim::{SimConfig, Simulator};

fn main() {
    let mut sim = Simulator::new(SimConfig {
        title: "knurl-sim - empty (close window to quit)".to_string(),
        ..Default::default()
    });

    // Nothing to draw; the loop just pumps events. Closing the window exits.
    sim.run(|_target, _msgs| {});
}
