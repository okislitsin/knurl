//! `knurl-sim` - a desktop simulator for the [`knurl`] embedded TUI library.
//!
//! It lets you prototype `knurl` screens on the desktop with no hardware. The
//! crucial point: it reuses the **exact** render path used on a real panel.
//!
//! ```text
//! SimulatorDisplay<BinaryColor>  →  GraphicsTarget (unchanged)  →  your Component screen
//! ```
//!
//! `SimulatorDisplay<BinaryColor>` is already a
//! `DrawTarget<Color = BinaryColor>`, and `knurl::graphics::GraphicsTarget` is
//! generic over that, so the simulator never defines its own `RenderTarget`.
//! It is just a std wrapper: a window, an event pump, a keyboard→`Msg` map, and
//! a tick timer.
//!
//! # Features
//! - `desktop` (default): SDL2 window via `embedded-graphics-simulator`.
//! - `web`: placeholder for a future WASM backend; builds without SDL2.
//!
//! # Example
//! ```ignore
//! use knurl_sim::{SimConfig, Simulator};
//! use knurl::{Area, Component, List};
//!
//! let mut sim = Simulator::new(SimConfig::default());
//! let items = ["Alpha", "Beta", "Gamma"];
//! let mut list = List::new(&items);
//! sim.run(|target, msgs| {
//!     for m in msgs { let _ = list.update(m); }
//!     list.view(target, Area::new(0, 0, 21, 12));
//! });
//! ```
//!
//! [`knurl`]: knurl_core

// The keymap is expressed in terms of the simulator's `Keycode`, so it lives
// behind the same feature gate as the desktop backend.
#[cfg(feature = "desktop")]
pub mod keymap;
#[cfg(feature = "desktop")]
pub use keymap::Keymap;

#[cfg(feature = "desktop")]
pub mod desktop;
#[cfg(feature = "desktop")]
pub use desktop::{
    ColorSimConfig, ColorSimTarget, ColorSimulator, Flow, Frame, IntoFlow, SimConfig, SimTarget,
    Simulator,
};

// Re-export the library so downstream code can depend solely on `knurl-sim`
// for prototyping. `core` is the facade itself: a screen written against
// `knurl_sim::core::…` and one written against `knurl::…` are the same screen.
pub use knurl as core;
pub use knurl::graphics;
