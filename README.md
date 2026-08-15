# knurl

**A grippy little TUI for embedded displays.**

`knurl` is a **pixel-native, `no_std`, allocation-free** TUI library for small
panels - OLED and TFT modules driven by microcontrollers like the RP2040. It
wears a [Bubble Tea](https://github.com/charmbracelet/bubbletea) / Charm-inspired
look (rounded chrome, a `>` cursor, an accented selection, dim everything else)
and is driven by a **rotary encoder with one button**. The guiding idea: *your
screen is a terminal* - a compact catalog of stack-only widgets, laid out in
pixels, that feels like a TUI on a 128×64 OLED.

<table>
<tr>
<td align="center"><b>OLED - 128×64 mono</b></td>
<td align="center"><b>TFT - 320×240 colour (Charm theme)</b></td>
</tr>
<tr>
<td><img src="docs/oled-menu.png" width="360" alt="OLED menu"></td>
<td><img src="docs/tft-menu.png" width="360" alt="TFT menu"></td>
</tr>
<tr>
<td><img src="docs/oled-tree.png" width="360" alt="OLED tree"></td>
<td><img src="docs/tft-chart.png" width="360" alt="TFT bar chart"></td>
</tr>
</table>

## The one idea worth knowing

Widgets draw onto an abstract `RenderTarget` and react to `Msg`s. They never know
what's behind the target - a real panel or a simulator window. That single seam
is what makes this true:

```
            ┌──────────────────────── the same Component code ───────────────────────┐
 firmware:  panel driver (SSD1306, ST7789, …)  →  Graphics/ColorGraphicsTarget  →  screens
 desktop:   SimulatorDisplay<BinaryColor|Rgb565> →  Graphics/ColorGraphicsTarget  →  screens
            └────────────────────────────────────────────────────────────────────────┘
```

`SimulatorDisplay` is already an `embedded-graphics` `DrawTarget`, and the
`GraphicsTarget` (mono) / `ColorGraphicsTarget` (colour) adapters are generic over
any such target - so the **simulator adds no new render path**. Screen code that
runs on hardware runs on your desktop, pixel-for-pixel.

The library is genuinely `no_std` and bare-metal portable - the machine proof:

```sh
rustup target add thumbv6m-none-eabi
cargo build -p knurl-core     --target thumbv6m-none-eabi
cargo build -p knurl-graphics --target thumbv6m-none-eabi
cargo build -p knurl-screens  --target thumbv6m-none-eabi   # the demo's own screens
```

That last line is the interesting one: `knurl-screens` is the demo application
itself - every screen you see below - and it links for bare metal, so a screen
file copies into a firmware project unchanged.

## Input model - a rotary encoder with one button

The target hardware is a **rotary encoder + push button**, so UIs are driven by
exactly three inputs and nothing else:

- **↑ / ↓** - rotate the encoder (move the cursor / change a value)
- **Space** - push the encoder button (select / edit / activate)

There is no Back, Left/Right, or text key - the device has none. **"Back" is a
selectable menu item**, never baked into a widget's data; the root menu's
**"Exit"** item asks the host to quit. The simulator reserves no quit key (Esc
does nothing); close the window (or pick "Exit") to leave.

## Architecture

knurl is **pixel-native**: there is no character grid. Coordinates and extents are
pixels (`Area` is `u16`), and widgets lay text out by asking the target for three
font metrics - `line_height()`, `char_width()`, `text_width(s)` - then positioning
glyphs by pixel. (Char-LCDs like the HD44780 are **out of scope**; knurl is
pixel-only.)

- **Semantic styling, per-target rendering.** A widget says *what* a piece of text
  is (`Style::{Normal, Accent, Muted, Danger, Focus, Inverted}`), never *how* to
  colour it. Each target decides: the mono `GraphicsTarget` maps styles to a 1-bit
  `Theme` (inversion / emphasis), the colour `ColorGraphicsTarget` maps them
  through a Charm `ColorTheme` (calm lilac selection, accented text, smooth bars -
  no hardcoded RGB). Semantic primitives like `draw_check` / `draw_radio` /
  `draw_bar` / `draw_spinner` let each target pixel-draw a real indicator.

- **DataProvider models.** Data-heavy widgets borrow a trait, not a fixed slice, so
  an app can back them with its own store (a fixed array, a ring buffer, generated
  rows) with no copying: `ListModel`, `TreeModel`, `TableModel`, `BarChartModel`,
  and `LinesModel` (the `Pager`, with a `write_line` variant for streaming data
  that is never stored whole). A plain `&[&str]` (etc.) still works via blanket
  impls.

- **Navigation - `Router` / `Nav`.** A fixed-depth, heap-free screen-history stack
  (`Router<Id, DEPTH>`): `push`/`pop`/`replace`, `current()`, `at_root()`. The app
  matches on `router.current()` to render a screen; a focusable "Back" item pops;
  "Back at the root" is the cue to exit.

- **Dirty + partial redraw.** Each `Component` carries a `Cell`-backed dirty flag
  set in `update()` only when state actually changes. The render loop *gates* on
  it: a frame with nothing dirty is skipped entirely, and a widget's `view()`
  self-clears and repaints **only its own area** - there is no global per-frame
  `clear()`. On a 320×240 panel a full repaint is ~150 KB over SPI, so animating
  one widget (a `Spinner`, a streaming `Pager`) repaints a few hundred bytes
  instead of the whole frame. (The win is hardware-measured; the simulator shows
  the gating.)

- **Desktop simulator (`knurl-sim`).** Mono and colour backends over
  `embedded-graphics-simulator`, sharing one event/render loop, plus the demos.

## Component catalog

Text & chrome: **Label**, **Title**, **Separator**, **Spacer**, **StatusBar**,
**Help**, **Dialog**, **Tabs**.
Data: **List**, **Tree**, **Table**, **BarChart**, **Pager** (with follow/tail).
Inputs: **Checkbox**, **Toggle**, **Counter**, **Slider**, **Picker**, **Radio**,
**TextInput**, and **Form** (a focus/edit-mode controller over `FormField`s).
Indicators: **Spinner**, **ProgressBar**, **LineGauge**, **Scrollbar**,
**Paginator**.
Layout: **VStack** / **HStack** (with `Constraint`), **Padded**, **Bordered**.

## Demos

The demo is an **application**, not a gallery: [`knurl-screens`](knurl-screens) is
a `no_std` crate with **one screen per file**, each implementing
[`Screen`](knurl-core/src/screen.rs) and owning its widgets. It covers the whole
catalog, but arranged as compositions - one form, **two forms sharing a layout**,
**three forms behind tabs**, a **list and a form on one screen** - because a
library of components that do not work together is not a library.

Two hosts run it, and differ only in what a host provides (panel, theme, chrome,
frame loop):

- **`oled`** - monochrome, tuned for a tiny SSD1306 (128×64 default; `128x128` via
  arg). Compact chrome for ~5 rows.
- **`tft`** - colour, 320×240 ST7789-class, the default Charm `ColorTheme`, with a
  persistent status-bar hint and a **live log** behind the Pager screen: a
  ring-buffer `LinesModel` that gains a line every few ticks (standing in for
  UART), in follow/tail mode. The screen rendering it is the same file a device
  builds against a `const` array.

Encoder model throughout; every page either fits or **scrolls** - nothing is
truncated. Both run on the dirty-gated partial-redraw loop.

<table>
<tr>
<td><img src="docs/oled-list.png" width="300" alt="OLED list"></td>
<td><img src="docs/oled-form.png" width="300" alt="OLED form"></td>
</tr>
<tr>
<td><img src="docs/tft-table.png" width="300" alt="TFT table"></td>
<td><img src="docs/tft-pager.png" width="300" alt="TFT realtime pager"></td>
</tr>
</table>

## Running the simulator

The desktop backend needs SDL2:

```sh
# macOS (Homebrew)
export LIBRARY_PATH="/opt/homebrew/lib:$LIBRARY_PATH"
export PKG_CONFIG_PATH="/opt/homebrew/opt/sdl2/lib/pkgconfig:$PKG_CONFIG_PATH"
# Debian/Ubuntu: sudo apt install libsdl2-dev

cargo run -p knurl-sim --example oled              # mono OLED, 128×64 (default)
cargo run -p knurl-sim --example oled -- 128x128   # mono OLED, taller variant
cargo run -p knurl-sim --example tft               # colour TFT, 320×240
```

Controls everywhere: **↑/↓** rotate, **Space** selects; "Back"/"Exit" are menu
items; close the window to quit.

### Regenerating the screenshots

The README images are produced **headlessly** (no SDL window - it renders to an
off-screen display and writes PNGs), so it works in CI:

```sh
cargo run -p knurl-sim --features desktop --example screenshots   # → docs/*.png
```

## Workspace layout

| Crate | `no_std` | Depends on | Role |
|-------|:--------:|------------|------|
| [`knurl-core`](knurl-core)         | ✅ (zero-dep) | - | Traits (`RenderTarget`, `Component`), `Msg`, `Style`, `Area`, `Router`, models, and every widget. |
| [`knurl-graphics`](knurl-graphics) | ✅ | `embedded-graphics` | `GraphicsTarget` / `ColorGraphicsTarget` adapters + `Theme` / `ColorTheme`. |
| [`knurl`](knurl)                   | ✅ | core (+ optional graphics) | Facade re-exporting the public API. |
| [`knurl-screens`](knurl-screens)   | ✅ | `knurl` only | The demo application: one `Screen` per file, shared by both demos, built for `thumbv6m` in CI. |
| [`knurl-sim`](knurl-sim)           | ❌ std | facade + `embedded-graphics-simulator` | Desktop simulator (mono + colour) and the demo hosts. |

`knurl-sim` is a workspace member but **excluded from `default-members`**, so a
plain `cargo test` from the root never needs SDL2.

## Testing

```sh
cargo test            # core + graphics widgets, host-side (no SDL2)
```

## License

MIT OR Apache-2.0.
