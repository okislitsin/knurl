//! Desktop backend: an SDL2 window driven by `embedded-graphics-simulator`.
//!
//! ## Architectural note
//!
//! This module deliberately does **not** define a new
//! [`RenderTarget`](knurl_core::RenderTarget). It wraps the existing renderers:
//!
//! ```text
//! SimulatorDisplay<BinaryColor>  →  GraphicsTarget        →  same screen code as hardware
//! SimulatorDisplay<Rgb565>       →  ColorGraphicsTarget   →  same screen code as hardware
//! ```
//!
//! Two backends share one event/render loop ([`Core`]). The monochrome
//! [`Simulator`] is the original API, unchanged for callers; [`ColorSimulator`]
//! is the colour twin. The per-frame callback receives a `&mut dyn RenderTarget`
//! so the loop body is written once and reused by both.

use std::time::{Duration, Instant};

use embedded_graphics::{
    mono_font::{MonoFont, ascii::FONT_6X10},
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
};
use embedded_graphics_simulator::{
    OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use knurl_core::{Msg, RenderTarget, Style};
use knurl_graphics::{ColorGraphicsTarget, ColorTheme, GraphicsTarget, Theme};

use crate::keymap::Keymap;

/// The monochrome `GraphicsTarget` - the *same* type firmware uses, wrapping a
/// `SimulatorDisplay<BinaryColor>` instead of a panel driver.
pub type SimTarget<'a> = GraphicsTarget<'a, SimulatorDisplay<BinaryColor>>;

/// The colour `ColorGraphicsTarget` over a `SimulatorDisplay<Rgb565>`.
pub type ColorSimTarget<'a> = ColorGraphicsTarget<'a, SimulatorDisplay<Rgb565>, Rgb565>;

/// What [`Simulator::run`] (or [`ColorSimulator::run`]) should do after a frame.
///
/// Lets the **app** - not the simulator - decide when to exit. The simulator
/// binds no quit key (the encoder hardware has none): exit is via closing the
/// window or returning [`Flow::Quit`] (e.g. from an in-app "Exit" item). The
/// per-frame callback may return either `()` (treated as [`Flow::Continue`]) or
/// a `Flow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running.
    Continue,
    /// Exit the loop and close the window.
    Quit,
}

/// Outcome of a **gated** per-frame callback ([`Simulator::run_gated`] /
/// [`ColorSimulator::run_gated`]).
///
/// The gated loop performs **no global screen clear** - each widget clears its
/// own area inside its [`view`](knurl_core::Component::view) (partial redraw).
/// The callback owns the Elm cycle and, after processing the frame's
/// messages, returns:
///
/// - [`Frame::Painted`] - at least one widget was dirty and repainted (only the
///   changed widgets' areas were touched); the loop presents the result.
/// - [`Frame::Skipped`] - nothing was dirty; the loop issues **no** view or
///   present, so the window keeps showing the previous frame. On hardware this is
///   the win: an idle frame pushes nothing over SPI.
/// - [`Frame::Quit`] - exit the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// One or more widgets repainted (only their areas) - present it.
    Painted,
    /// Nothing changed - skip view/present entirely.
    Skipped,
    /// Exit the loop and close the window.
    Quit,
}

/// Allows a `run` callback to return `()` or [`Flow`], so simple callbacks stay
/// terse while interactive ones can request a quit.
pub trait IntoFlow {
    fn into_flow(self) -> Flow;
}

impl IntoFlow for () {
    fn into_flow(self) -> Flow {
        Flow::Continue
    }
}

impl IntoFlow for Flow {
    fn into_flow(self) -> Flow {
        self
    }
}

// ── Configs ─────────────────────────────────────────────────────────────────

/// Configuration for a monochrome [`Simulator`].
///
/// Build with [`SimConfig::default`] and override fields, e.g.
/// `SimConfig { scale: 4, ..Default::default() }`.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Panel width in pixels (default 128).
    pub width: u32,
    /// Panel height in pixels (default 128).
    pub height: u32,
    /// Integer zoom of the window relative to the panel (default 4).
    pub scale: u32,
    /// Monospace font used by [`GraphicsTarget`] (default [`FONT_6X10`]).
    pub font: MonoFont<'static>,
    /// Window title.
    pub title: String,
    /// Keyboard → [`Msg`] mapping (default [`Keymap::encoder`]).
    pub keymap: Keymap,
    /// Monochrome theme controlling per-style inversion and blink phase.
    pub theme: Theme,
    /// Interval between [`Msg::Tick`] events, in milliseconds (default 100).
    pub tick_ms: u64,
    /// Toggle [`Theme`] blink phase every this many ticks (default 5).
    pub blink_ticks: u32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            width: 128,
            height: 128,
            scale: 4,
            font: FONT_6X10,
            title: "knurl-sim".to_string(),
            keymap: Keymap::encoder(),
            theme: Theme::new(),
            tick_ms: 100,
            blink_ticks: 5,
        }
    }
}

/// Configuration for a colour [`ColorSimulator`].
///
/// Defaults to a 320×240 ST7789-class panel in [`Rgb565`].
#[derive(Debug, Clone)]
pub struct ColorSimConfig {
    /// Panel width in pixels (default 320).
    pub width: u32,
    /// Panel height in pixels (default 240).
    pub height: u32,
    /// Integer zoom of the window relative to the panel (default 2).
    pub scale: u32,
    /// Monospace font used by [`ColorGraphicsTarget`] (default [`FONT_6X10`]).
    pub font: MonoFont<'static>,
    /// Window title.
    pub title: String,
    /// Keyboard → [`Msg`] mapping (default [`Keymap::encoder`]).
    pub keymap: Keymap,
    /// Colour theme controlling per-style `(fg, bg)` colours.
    pub theme: ColorTheme<Rgb565>,
    /// Interval between [`Msg::Tick`] events, in milliseconds (default 100).
    pub tick_ms: u64,
    /// Tick divisor reserved for parity with the mono loop (colour has no blink).
    pub blink_ticks: u32,
}

impl Default for ColorSimConfig {
    fn default() -> Self {
        Self {
            width: 320,
            height: 240,
            scale: 2,
            font: FONT_6X10,
            title: "knurl-sim (color)".to_string(),
            keymap: Keymap::encoder(),
            theme: ColorTheme::default(),
            tick_ms: 100,
            blink_ticks: 5,
        }
    }
}

// ── Backend abstraction ───────────────────────────────────────────────────────

/// A render backend the shared [`Core`] loop drives. Hides the concrete colour
/// type / target so the loop is written once for both mono and colour.
trait Backend {
    /// Per-blink-tick hook (mono toggles its blink phase; colour is a no-op).
    fn on_tick(&mut self);
    /// Clear the whole display to the background.
    fn clear(&mut self);
    /// Build this backend's `RenderTarget` for the frame and run `on_frame`.
    fn render(
        &mut self,
        msgs: &[Msg],
        on_frame: &mut dyn FnMut(&mut dyn RenderTarget, &[Msg]) -> Flow,
    ) -> Flow;
    /// Like [`render`](Backend::render) but for the gated loop: the callback
    /// returns a [`Frame`] and clears the target itself when it paints.
    fn render_gated(
        &mut self,
        msgs: &[Msg],
        on_frame: &mut dyn FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    ) -> Frame;
    /// Push the display contents to the window.
    fn present(&self, window: &mut Window);
}

/// Monochrome backend: `SimulatorDisplay<BinaryColor>` + [`GraphicsTarget`].
struct MonoBackend {
    display: SimulatorDisplay<BinaryColor>,
    font: MonoFont<'static>,
    theme: Theme,
}

impl Backend for MonoBackend {
    fn on_tick(&mut self) {
        self.theme.toggle_blink();
    }

    fn clear(&mut self) {
        let _ = self.display.clear(BinaryColor::Off);
    }

    fn render(
        &mut self,
        msgs: &[Msg],
        on_frame: &mut dyn FnMut(&mut dyn RenderTarget, &[Msg]) -> Flow,
    ) -> Flow {
        let mut target = GraphicsTarget::new(&mut self.display, self.font).with_theme(self.theme);
        on_frame(&mut target, msgs)
    }

    fn render_gated(
        &mut self,
        msgs: &[Msg],
        on_frame: &mut dyn FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    ) -> Frame {
        let mut target = GraphicsTarget::new(&mut self.display, self.font).with_theme(self.theme);
        on_frame(&mut target, msgs)
    }

    fn present(&self, window: &mut Window) {
        window.update(&self.display);
    }
}

/// Colour backend: `SimulatorDisplay<Rgb565>` + [`ColorGraphicsTarget`].
struct ColorBackend {
    display: SimulatorDisplay<Rgb565>,
    font: MonoFont<'static>,
    theme: ColorTheme<Rgb565>,
}

impl Backend for ColorBackend {
    fn on_tick(&mut self) {
        // The colour theme has no blink phase - nothing to advance.
    }

    fn clear(&mut self) {
        let _ = self.display.clear(self.theme.background(Style::Normal));
    }

    fn render(
        &mut self,
        msgs: &[Msg],
        on_frame: &mut dyn FnMut(&mut dyn RenderTarget, &[Msg]) -> Flow,
    ) -> Flow {
        let mut target =
            ColorGraphicsTarget::new(&mut self.display, self.font).with_theme(self.theme);
        on_frame(&mut target, msgs)
    }

    fn render_gated(
        &mut self,
        msgs: &[Msg],
        on_frame: &mut dyn FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    ) -> Frame {
        let mut target =
            ColorGraphicsTarget::new(&mut self.display, self.font).with_theme(self.theme);
        on_frame(&mut target, msgs)
    }

    fn present(&self, window: &mut Window) {
        window.update(&self.display);
    }
}

// ── Shared loop core ──────────────────────────────────────────────────────────

/// Window + input + timing shared by both backends. Owns the single event/render
/// loop so it is never duplicated per colour type.
struct Core {
    window: Window,
    keymap: Keymap,
    tick_interval: Duration,
    blink_ticks: u32,
}

impl Core {
    /// Runs until the window closes or the callback returns [`Flow::Quit`].
    ///
    /// This is the **immediate-mode** loop: every frame clears, redraws and
    /// presents. For the dirty-gated loop that skips unchanged frames, see
    /// [`run_gated`](Core::run_gated).
    fn run<F, R>(&mut self, backend: &mut dyn Backend, mut on_frame: F)
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> R,
        R: IntoFlow,
    {
        let mut last_tick = Instant::now();
        let mut ticks_since_blink = 0u32;

        // One-time clean slate: clear the whole screen once before the first
        // frame (there is no per-frame global clear; widgets self-clear their
        // own areas thereafter).
        backend.clear();

        // Render the initial frame *before* polling events: the simulator creates
        // its SDL window lazily on the first `update`, and `events()` panics if
        // called beforehand. Rendering-then-input is also exactly knurl's Elm
        // cycle, so the first frame is drawn with no messages.
        if self.draw_frame(backend, &[], &mut on_frame) == Flow::Quit {
            return;
        }

        loop {
            match self.poll(&mut last_tick, &mut ticks_since_blink, backend) {
                None => break,
                Some(msgs) => {
                    if self.draw_frame(backend, &msgs, &mut on_frame) == Flow::Quit {
                        break;
                    }
                }
            }

            // Yield the CPU; the tick timer governs logical timing.
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    /// The **dirty-gated** loop: a frame whose callback returns [`Frame::Skipped`]
    /// issues no clear, view or present at all - the window keeps the last frame.
    ///
    /// Messages are still delivered to the callback every frame (so `update` runs
    /// and ticks advance animations); only the *paint* is gated. There is no
    /// global clear - each widget clears its own area inside `view` (partial
    /// redraw), so a painted frame touches only the changed widgets (see [`Frame`]).
    fn run_gated<F>(&mut self, backend: &mut dyn Backend, mut on_frame: F)
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    {
        let mut last_tick = Instant::now();
        let mut ticks_since_blink = 0u32;

        // One-time clean slate (see `run`): clear once, then rely on per-widget
        // self-clear. After this, a skipped frame touches no pixels.
        backend.clear();

        // First frame always presents (creating the SDL window lazily, before any
        // `events()` call). Components start dirty, so the callback paints it.
        if self.gated_frame(backend, &[], true, &mut on_frame) == Flow::Quit {
            return;
        }

        loop {
            match self.poll(&mut last_tick, &mut ticks_since_blink, backend) {
                None => break,
                Some(msgs) => {
                    if self.gated_frame(backend, &msgs, false, &mut on_frame) == Flow::Quit {
                        break;
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(16));
        }
    }

    /// Collects this frame's input + tick messages. Returns `None` when the window
    /// was closed (the caller should stop). Shared by both loops.
    fn poll(
        &mut self,
        last_tick: &mut Instant,
        ticks_since_blink: &mut u32,
        backend: &mut dyn Backend,
    ) -> Option<Vec<Msg>> {
        let mut msgs: Vec<Msg> = Vec::new();

        for event in self.window.events() {
            match event {
                SimulatorEvent::Quit => return None,
                SimulatorEvent::KeyDown { keycode, .. } => {
                    // No key is reserved for quitting - the encoder model has no
                    // such input. Exit is via window close or `Flow::Quit`.
                    if let Some(msg) = self.keymap.map(keycode) {
                        msgs.push(msg);
                    }
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= self.tick_interval {
            *last_tick = Instant::now();
            msgs.push(Msg::Tick);
            *ticks_since_blink += 1;
            if *ticks_since_blink >= self.blink_ticks {
                *ticks_since_blink = 0;
                backend.on_tick();
            }
        }

        Some(msgs)
    }

    /// Let `on_frame` draw via the backend's target, then present.
    ///
    /// There is **no per-frame global clear** - widgets clear their own areas
    /// inside `view` (partial redraw). The whole screen is cleared once before
    /// the first frame (see [`run`](Core::run)).
    fn draw_frame<F, R>(
        &mut self,
        backend: &mut dyn Backend,
        msgs: &[Msg],
        on_frame: &mut F,
    ) -> Flow
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> R,
        R: IntoFlow,
    {
        let flow = backend.render(msgs, &mut |t, m| on_frame(t, m).into_flow());
        backend.present(&mut self.window);
        flow
    }

    /// Runs one gated frame: the callback updates, then either paints (and we
    /// present) or skips (and we do nothing). No global clear - each widget
    /// clears its own area inside `view` (partial redraw), so a painted frame
    /// touches only the changed widgets and a skipped frame touches nothing.
    fn gated_frame<F>(
        &mut self,
        backend: &mut dyn Backend,
        msgs: &[Msg],
        first: bool,
        on_frame: &mut F,
    ) -> Flow
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    {
        match backend.render_gated(msgs, &mut |t, m| on_frame(t, m)) {
            Frame::Quit => Flow::Quit,
            Frame::Painted => {
                backend.present(&mut self.window);
                Flow::Continue
            }
            Frame::Skipped => {
                // Safety net: ensure the window exists after the first frame even
                // if the callback somehow painted nothing.
                if first {
                    backend.present(&mut self.window);
                }
                Flow::Continue
            }
        }
    }
}

/// Builds an SDL window from a title and integer scale.
fn make_window(title: &str, scale: u32) -> Window {
    Window::new(title, &OutputSettingsBuilder::new().scale(scale).build())
}

// ── Monochrome simulator (original API) ─────────────────────────────────────

/// A live SDL2 simulator window over a monochrome [`SimulatorDisplay`].
///
/// Renders through the unchanged [`GraphicsTarget`], so any
/// [`Component`](knurl_core::Component) runs exactly as on a physical panel.
pub struct Simulator {
    core: Core,
    backend: MonoBackend,
}

impl Simulator {
    /// Opens a window sized from `config`.
    pub fn new(config: SimConfig) -> Self {
        let display = SimulatorDisplay::<BinaryColor>::new(Size::new(config.width, config.height));
        let window = make_window(&config.title, config.scale);
        Self {
            core: Core {
                window,
                keymap: config.keymap,
                tick_interval: Duration::from_millis(config.tick_ms.max(1)),
                blink_ticks: config.blink_ticks.max(1),
            },
            backend: MonoBackend {
                display,
                font: config.font,
                theme: config.theme,
            },
        }
    }

    /// Mutable access to the current [`Theme`] (e.g. to drive blink manually).
    pub fn theme_mut(&mut self) -> &mut Theme {
        &mut self.backend.theme
    }

    /// Builds a fresh [`GraphicsTarget`] over the display with the current theme.
    pub fn target(&mut self) -> SimTarget<'_> {
        GraphicsTarget::new(&mut self.backend.display, self.backend.font)
            .with_theme(self.backend.theme)
    }

    /// Runs the event/render loop until the window closes.
    ///
    /// `on_frame` wires up the Elm cycle - `for m in msgs { c.update(m) }` then
    /// `c.view(target, area)`. The target is a `&mut dyn RenderTarget`. Return
    /// `()` to keep running, or [`Flow::Quit`] to exit.
    pub fn run<F, R>(&mut self, on_frame: F)
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> R,
        R: IntoFlow,
    {
        self.core.run(&mut self.backend, on_frame);
    }

    /// Runs the **dirty-gated** event/render loop until the window closes.
    ///
    /// Like [`run`](Simulator::run), but with **no global screen clear** - each
    /// widget repaints only its own area when dirty (partial redraw). The
    /// callback owns the Elm cycle and returns a [`Frame`]: after
    /// `for m in msgs { c.update(m) }`, if nothing is dirty return
    /// [`Frame::Skipped`]; otherwise call each widget's `c.view(..)` (each
    /// self-gates and clears its own area) and return [`Frame::Painted`] (or
    /// [`Frame::Quit`]). On a structural transition (nav / modal close) the app
    /// clears the affected region and calls [`mark_dirty`](knurl_core::Component::mark_dirty)
    /// so it repaints cleanly. A skipped frame issues no draw ops - on hardware,
    /// nothing over SPI.
    pub fn run_gated<F>(&mut self, on_frame: F)
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    {
        self.core.run_gated(&mut self.backend, on_frame);
    }
}

// ── Colour simulator ────────────────────────────────────────────────────────

/// A live SDL2 simulator window over a colour [`SimulatorDisplay`] ([`Rgb565`]).
///
/// Renders the *same* [`Component`](knurl_core::Component) screens as
/// [`Simulator`], through [`ColorGraphicsTarget`] + [`ColorTheme`]. Defaults to
/// a 320×240 ST7789-class panel.
pub struct ColorSimulator {
    core: Core,
    backend: ColorBackend,
}

impl ColorSimulator {
    /// Opens a colour window sized from `config`.
    pub fn new(config: ColorSimConfig) -> Self {
        let display = SimulatorDisplay::<Rgb565>::new(Size::new(config.width, config.height));
        let window = make_window(&config.title, config.scale);
        Self {
            core: Core {
                window,
                keymap: config.keymap,
                tick_interval: Duration::from_millis(config.tick_ms.max(1)),
                blink_ticks: config.blink_ticks.max(1),
            },
            backend: ColorBackend {
                display,
                font: config.font,
                theme: config.theme,
            },
        }
    }

    /// The active colour theme.
    pub fn theme(&self) -> &ColorTheme<Rgb565> {
        &self.backend.theme
    }

    /// Builds a fresh [`ColorGraphicsTarget`] over the display with the theme.
    pub fn target(&mut self) -> ColorSimTarget<'_> {
        ColorGraphicsTarget::new(&mut self.backend.display, self.backend.font)
            .with_theme(self.backend.theme)
    }

    /// Runs the event/render loop until the window closes. Identical contract to
    /// [`Simulator::run`] - same loop, colour target.
    pub fn run<F, R>(&mut self, on_frame: F)
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> R,
        R: IntoFlow,
    {
        self.core.run(&mut self.backend, on_frame);
    }

    /// Runs the **dirty-gated** event/render loop - the colour twin of
    /// [`Simulator::run_gated`]. Same contract: the callback returns a [`Frame`]
    /// and only changed frames are painted.
    pub fn run_gated<F>(&mut self, on_frame: F)
    where
        F: FnMut(&mut dyn RenderTarget, &[Msg]) -> Frame,
    {
        self.core.run_gated(&mut self.backend, on_frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics_simulator::sdl2::Keycode;

    /// Regression: the default config uses the encoder keymap - Space selects,
    /// rotation moves, and nothing else (no Back/arrows/quit key) reaches the UI.
    #[test]
    fn default_is_encoder_model() {
        let cfg = SimConfig::default();
        assert_eq!(cfg.keymap.map(Keycode::Up), Some(Msg::Up));
        assert_eq!(cfg.keymap.map(Keycode::Down), Some(Msg::Down));
        assert_eq!(cfg.keymap.map(Keycode::Space), Some(Msg::Select));
        assert_eq!(cfg.keymap.map(Keycode::Escape), None);
        assert_eq!(cfg.keymap.map(Keycode::Return), None);
        assert_eq!(cfg.keymap.map(Keycode::Left), None);
        assert_eq!(cfg.keymap.map(Keycode::Right), None);
    }

    /// `IntoFlow` keeps `()`-returning callbacks meaning "continue".
    #[test]
    fn unit_callback_continues() {
        assert_eq!(().into_flow(), Flow::Continue);
        assert_eq!(Flow::Quit.into_flow(), Flow::Quit);
    }

    /// Colour defaults: 320×240 ST7789 panel, encoder keymap, default palette.
    #[test]
    fn color_config_defaults() {
        let c = ColorSimConfig::default();
        assert_eq!((c.width, c.height), (320, 240));
        assert_eq!(c.keymap.map(Keycode::Space), Some(Msg::Select));
        assert_eq!(
            c.theme.resolve(Style::Normal),
            (Rgb565::WHITE, Rgb565::BLACK)
        );
        assert_eq!(
            c.theme.resolve(Style::Accent),
            (Rgb565::BLACK, Rgb565::BLUE)
        );
    }
}
