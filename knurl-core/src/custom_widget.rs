//! # Writing your own widget
//!
//! The catalogue covers what most panels need, and then it does not cover the
//! one thing yours does: a dial, a level meter, a device icon, a setpoint your
//! product is actually about. This is the contract for that - what you have to
//! bring, what you get for free, and what it costs when a clause is skipped.
//!
//! There is a worked example outside the library, written against nothing but
//! the `knurl` facade, in the demo application: `knurl-screens/src/thermostat.rs`.
//! Everything below is visible there in one file, with tests beside it in
//! `knurl-screens/tests/thermostat.rs`.
//!
//! ## Three levels of freedom, cheapest first
//!
//! Pick the lowest one that answers, because each step up is more that you own.
//!
//! ### 1. A picture: [`Canvas`](crate::Canvas)
//!
//! If nothing about it is *driven* - a gauge, a sparkline, a logo, an arrow
//! following a value - it is a drawing, not a widget. [`Canvas`] takes a closure
//! and brings the four obligations below with it:
//!
//! ```
//! use knurl_core::{Area, Canvas, Component, RenderTarget, Style};
//!
//! let arrow = Canvas::new(|t: &mut dyn RenderTarget, a: Area| {
//!     t.draw_line(a.x, a.y + a.h / 2, a.x + a.w - 1, a.y + a.h / 2, Style::Accent);
//! });
//! assert!(arrow.dirty(), "a fresh canvas owes its first paint");
//! ```
//!
//! What it cannot do is hold state or take the focus: a closure has neither a
//! name nor a cursor. Its dirty flag is therefore the owner's business - see
//! [`Canvas`] for the two honest ways to run one.
//!
//! ### 2. Something the user drives: a [`Component`](crate::Component) of your own
//!
//! State plus input means a type. That is the rest of this page.
//!
//! ### 3. Pixels the portable primitives cannot express
//!
//! Arcs, images, your own font. `knurl-graphics` hands over the raw
//! `embedded-graphics` `DrawTarget`, clipped to your area, through
//! `GraphicsTarget::clipped(area)`. The cost is real and worth knowing before
//! you reach for it: what goes through a raw target is invisible to the
//! semantic layer, so it is a colour on a colour panel and *nothing* on a mono
//! one, no theme touches it, and a recording target sees an opaque blob instead
//! of the shape you drew. Reach for it last, and for as few pixels as possible.
//!
//! ## The `Component` contract, clause by clause
//!
//! Four obligations. Here they are in one widget - a level the encoder turns:
//!
//! ```
//! use core::cell::Cell;
//! use knurl_core::{Area, Component, Msg, Outcome, RenderTarget, Style};
//!
//! pub struct Level {
//!     value: u8,
//!     max: u8,
//!     focused: bool,
//!     dirty: Cell<bool>,
//! }
//!
//! impl Level {
//!     pub const fn new(max: u8) -> Self {
//!         // Starts dirty, so the first frame paints.
//!         Self { value: 0, max, focused: false, dirty: Cell::new(true) }
//!     }
//!     pub fn value(&self) -> u8 { self.value }
//! }
//!
//! impl Component for Level {
//!     fn update(&mut self, msg: &Msg) -> Outcome {
//!         match msg {
//!             // 1. The flag goes up only where the state actually moved.
//!             Msg::Up if self.value < self.max => {
//!                 self.value += 1;
//!                 self.dirty.set(true);
//!                 Outcome::Consumed
//!             }
//!             Msg::Down if self.value > 0 => {
//!                 self.value -= 1;
//!                 self.dirty.set(true);
//!                 Outcome::Consumed
//!             }
//!             // 2. A choice the application acts on - reported whether or not
//!             //    it changes a pixel.
//!             Msg::Select => Outcome::Activated,
//!             // 3. An edge, or an event that was never ours: unspent, so the
//!             //    container can give it to the next zone.
//!             _ => Outcome::Ignored,
//!         }
//!     }
//!
//!     fn focus(&mut self) { self.focused = true; self.dirty.set(true); }
//!     fn blur(&mut self) { self.focused = false; self.dirty.set(true); }
//!
//!     fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
//!         // 4. The area is non-empty, never necessarily useful.
//!         let lh = target.line_height().max(1);
//!         if area.h < lh || area.w < 8 {
//!             return;
//!         }
//!         if self.focused {
//!             target.fill_band(Area::new(area.x, area.y, area.w, lh), Style::Focus);
//!         }
//!         let permille = self.value as u16 * 1000 / self.max.max(1) as u16;
//!         target.draw_bar(Area::new(area.x, area.y, area.w, lh), permille, Style::Accent);
//!     }
//!
//!     fn dirty(&self) -> bool { self.dirty.get() }
//!     fn mark_clean(&self) { self.dirty.set(false); }
//!     fn mark_dirty(&self) { self.dirty.set(true); }
//! }
//!
//! let mut level = Level::new(8);
//! assert_eq!(level.update(&Msg::Down), Outcome::Ignored, "already at the floor");
//! assert_eq!(level.update(&Msg::Up), Outcome::Consumed);
//! assert_eq!(level.value(), 1);
//! ```
//!
//! ### `draw` paints, `view` gates
//!
//! [`view`](crate::Component::view) is **provided**, and it is where the
//! gating lives: a clean widget returns without drawing anything at all, and a
//! dirty one has its area cleared, gets [`draw`](crate::Component::draw)
//! called, and is marked clean. So a leaf widget writes `draw` and never
//! touches `view`. Override `view`
//! only to build a *container* - one that dispatches to children which gate
//! themselves, without clearing over them (that is what `Padded` and `Bordered`
//! do).
//!
//! Two consequences worth having in mind:
//!
//! * `draw` takes `&self`. Anything it has to remember - the row count it just
//!   worked out from the area, for instance - goes in a [`Cell`](core::cell::Cell),
//!   which is how [`List`](crate::List) tells the next `update` how big a page is.
//! * `view` marks the widget clean whether or not `draw` found room to paint.
//!   A widget that bails on a too-small area is *clean* afterwards, so if your
//!   layout can grow at runtime, mark the widget dirty where it grows.
//!
//! ### The dirty flag is set only by a real change
//!
//! This is the clause that is skipped most often, and skipping it is invisible:
//! the picture is right, the widget simply repaints for the rest of the
//! session. Guard on the state, not on the event - a clamped `Up` at the top of
//! a range must leave the flag alone, and so must a setter that lands on the
//! value it already held. `Scrollbar::set` and `Tabs::set_focused` are
//! re-asserted every frame by design, which is why "changed" there has to mean
//! changed.
//!
//! ### Guard the area
//!
//! `view` guarantees a non-empty area, not a useful one. A screen hands over
//! whatever its layout produced, and four pixels is a valid answer. Check for
//! the room you need at the top of `draw` and return: the widget has already
//! had its area cleared, so returning leaves it blank rather than broken.
//!
//! ### `Outcome` is about the event, never the pixels
//!
//! [`Ignored`](crate::Outcome::Ignored) is a **routing signal, not an error**:
//! the event was not spent, so the container may hand it to the next focus zone
//! or up to the application. It is the only reason a cursor can leave your widget
//! at all - a widget that consumes `Up` and `Down` unconditionally traps the
//! encoder, and on hardware with no Back key that is a screen the user cannot
//! leave.
//!
//! [`Activated`](crate::Outcome::Activated) is a choice the application
//! usually acts on. *Which* choice is asked of the widget afterwards
//! ([`List::selected`](crate::List::selected) and friends), which is what keeps
//! the type `Copy` and payload-free.
//!
//! And never derive an outcome from the dirty flag. They answer different
//! questions: a [`Dialog`](crate::Dialog) confirming on `Select` repaints
//! nothing and is `Activated`; a `Tick` that advances a spinner repaints and is
//! `Consumed`.
//!
//! ## Styles, not colours
//!
//! Every primitive takes a [`Style`](crate::Style) - what the pixels *mean* -
//! and no primitive takes a colour. Your widget says `Danger`, and the target
//! decides: a lilac-on-dark theme on a colour TFT, inversion on a 1-bit OLED, a
//! recorded enum in a test.
//!
//! What that buys, concretely: the same file renders on both panels; a user's
//! theme reaches your widget without your knowing; and a test can assert
//! `Op::Text { style: Style::Danger, .. }` instead of comparing pixels. What it
//! costs: you cannot pick a shade. On monochrome, several styles map to the
//! same ink, so carry the meaning in **geometry** as well - a band, a marker, a
//! thicker rule. That is why the focus language is a full-width band and why
//! the active tab is 3px where the others are 1px.
//!
//! ## What a frame costs
//!
//! A widget's `view` clears its own area, the target unions every draw call
//! into one box, and the application pushes that box to the panel
//! ([`take_dirty_rect`](crate::RenderTarget::take_dirty_rect)). So a needless
//! repaint is not free RAM traffic - it is bytes on the bus, every frame,
//! forever. Measured on the demo at 320x240, 2 bytes/pixel (a full frame is
//! 153 600 B):
//!
//! | what happened | region | bytes | of a frame |
//! |---|---|---|---|
//! | an idle frame, nothing dirty | – | 0 B | 0 % |
//! | one step of a value being edited | 312x10 | 6 240 B | 4.1 % |
//! | one spinner tick | 312x10 | 6 240 B | 4.1 % |
//! | a tick moving three indicators five rows apart | 312x50 | 31 200 B | 20.3 % |
//! | moving the cursor in a full-screen list | 312x206 | 128 544 B | 83.7 % |
//! | opening another screen | 320x239 | 152 960 B | 99.6 % |
//!
//! Two things to read out of that table. The granularity is the **widget**, so
//! a big widget costs a big region and there is nothing clever to do about it.
//! And the region is **one rectangle**, not a list, so three small widgets far
//! apart cost the box that contains them - which is a reason to keep what
//! animates together, near each other.
//!
//! The mistake that undoes all of it is building a widget inside `draw`:
//!
//! ```
//! # use knurl_core::{Area, Component, RenderTarget, Title};
//! # struct S { name: &'static str }
//! # impl S {
//! fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
//!     // Wrong: a new Title every frame, and a fresh widget is dirty by
//!     // construction - so this row repaints, and re-dirties, forever.
//!     Title::new(self.name).view(target, area);
//! }
//! # }
//! ```
//!
//! Nothing looks wrong on the panel, which is why it survives. Keep the widget
//! in a field and change it with its setter; the setter is what marks it dirty.
//! The demo's Indicators screen used to do it the wrong way and reported 84% of
//! the panel on an idle tick, against 4% for the same animation now.
//!
//! ## Widgets over data you do not own
//!
//! If your widget borrows a model - somebody else's array, a ring buffer, a
//! sensor store - then the data can change without the widget being told, and
//! reading the whole model every frame to find out costs more than the repaint
//! would. There are three answers, in the order worth trying them:
//!
//! 1. **The model keeps a revision.** Give it
//!    [`revision`](crate::ListModel::revision) - a `u32` that changes when the
//!    content does - and the widget compares it against the one on screen. This
//!    is the only answer nobody can forget. It costs the model one word, or
//!    nothing at all if it already has a counter or a length to hand back.
//! 2. **Build the widget where it is drawn.** Fresh means dirty, so it repaints
//!    every frame - which is right for a readout that really does change every
//!    tick, and costs its own area and nothing else.
//! 3. **Keep it in a field and call
//!    [`mark_dirty`](crate::Component::mark_dirty)** wherever the data changed.
//!    Honest and cheap, and the one that goes stale the day somebody forgets.
//!    The symptom is only a picture that is out of date: nothing crashes and
//!    nothing logs.
//!
//! [`DataGate`](crate::DataGate) is where a widget keeps its flag and the
//! revision it last painted, so answering (1) is three lines - the same three
//! that [`List`](crate::List) uses:
//!
//! ```
//! # use knurl_core::{Component, DataGate, ListModel};
//! # struct W<'a, M: ListModel + ?Sized> { model: &'a M, gate: DataGate }
//! # impl<M: ListModel + ?Sized> W<'_, M> {
//! fn dirty(&self) -> bool { self.gate.is_dirty(self.model.revision()) }
//! fn mark_clean(&self) { self.gate.mark_clean(self.model.revision()); }
//! fn mark_dirty(&self) { self.gate.mark_dirty(); }
//! # }
//! ```
//!
//! A model that keeps no revision returns the same number forever, so the
//! comparison never fires and the widget falls back to (2) or (3) exactly as
//! before. Nothing is imposed on a `const` array of strings.
//!
//! ## Becoming a zone, becoming a field
//!
//! **A zone is free.** [`FocusZone`](crate::FocusZone) has a blanket
//! implementation for every [`Component`](crate::Component), so your widget can
//! go straight into a [`Screen`](crate::Screen)'s zone list with nothing
//! declared. Override [`focusable`](crate::Component::focusable) to `false` if
//! the cursor has no business stopping on it - a chain then steps over it
//! instead of costing the user a click on a picture.
//!
//! **A field is a choice.** Implement [`FormField`](crate::FormField) when your
//! widget belongs in a stack of labelled rows that a [`Form`](crate::Form)
//! drives: the form owns the edit mode, so `editable()` decides whether
//! `Select` enters an edit (with `Up`/`Down` going to the field) or acts
//! immediately. Override `height` if you need more than one text row, and note
//! that the height may depend on state - [`TextInput`](crate::TextInput) is two
//! rows while editing and one otherwise.
//!
//! The convention for an editable field: `Select` reports
//! [`Ignored`](crate::Outcome::Ignored) from the field itself, because entering and
//! leaving the edit belongs to the form. [`Counter`](crate::Counter),
//! `Slider` and `Picker` all do this.
//!
//! ## Traps we have already walked into
//!
//! * **[`Screen::enter`](crate::Screen::enter) cannot be overridden.** A Rust
//!   `impl` cannot call the default body it replaces, so an override would
//!   silently drop the focus placement. Put your reset in
//!   [`on_enter`](crate::Screen::on_enter). The same applies to any provided
//!   method that does bookkeeping.
//! * **The repaint cascade walks zones.** [`Screen::enter`](crate::Screen::enter)
//!   and [`invalidate`](crate::Screen::invalidate) mark every zone the screen
//!   listed - so a widget that is *not* a zone (a canvas, a caption, a row
//!   inside a hand-drawn window) has to be marked by hand, usually one line in
//!   `on_enter`.
//! * **[`FocusZone`](crate::FocusZone) names its methods differently on
//!   purpose** - `handle` / `enter` / `leave` / `invalidate`, not `update` /
//!   `focus` / `blur` / `mark_dirty`. The blanket implementation covers every
//!   `Component`, so shared names would make every call on a concrete widget
//!   ambiguous (`E0034`) whenever both traits are in scope.
//! * **Do not reach for `display_mut()`** when `clipped(area)` will do. An
//!   unclipped raw target cannot be followed, so it conservatively dirties the
//!   whole panel - a full frame per redraw.
//! * **Do not draw pixel by pixel in a loop.** Every primitive call goes
//!   through `dyn` dispatch and may become a bus transaction. A 40x20 shape
//!   painted with [`set_pixel`](crate::RenderTarget::set_pixel) is 800 of both,
//!   where one [`draw_bitmap`](crate::RenderTarget::draw_bitmap) is a handful of
//!   runs.
//!
//! ## The checklist
//!
//! * `update` returns `Consumed` on a real change, `Ignored` at the edges and
//!   for events that are not yours, `Activated` for a choice.
//! * The dirty flag goes up only where the state actually moved.
//! * `draw` guards the area it needs and returns if it is not there.
//! * Every primitive takes a [`Style`](crate::Style); nothing takes a colour.
//! * The widget lives in a field, and is never built inside `draw`.
//! * If it borrows a model, the model has a
//!   [`revision`](crate::ListModel::revision) - or its owner has a
//!   `mark_dirty()`, deliberately.
//!
//! [`Canvas`]: crate::Canvas
