# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/okislitsin/knurl/compare/knurl-core-v0.5.0...knurl-core-v0.5.1) - 2026-08-18

### Added

- *(core)* a model can say when its data changed

### Fixed

- *(screens)* the last three screens stop repainting what did not change
- *(core)* the cursor follows a model that shrank under it
- *(core)* a widget too small to paint is still owed its paint

### Other

- *(sim)* a screenshot matrix of every widget in every state
- *(core)* a seeded sweep over every widget and every composition
- the contract for writing a widget of your own

## [0.5.0](https://github.com/okislitsin/knurl/compare/knurl-core-v0.4.0...knurl-core-v0.5.0) - 2026-08-17

### Added

- *(core)* a target that can say which pixels moved

### Fixed

- *(core)* the always-dirty widgets get the gate everybody else has

### Other

- how to send the region, and the habit that ruins it

## [0.4.0](https://github.com/okislitsin/knurl/compare/knurl-core-v0.3.0...knurl-core-v0.4.0) - 2026-08-17

### Added

- *(core)* Tree's leaf cursor is a Marker like everybody else's
- *(core)* Canvas - free-hand drawing without declaring a type
- *(core)* four primitives so a widget can draw for itself

### Fixed

- *(core)* a dialog with no buttons has nothing to confirm
- *(core)* the tab strip says where the encoder is, and which tab is on

### Other

- *(readme)* free-hand primitives, Canvas and the escape hatch
- *(core)* Msg's extra buttons are an extension point, not dead code

## [0.3.0](https://github.com/okislitsin/knurl/compare/knurl-core-v0.2.0...knurl-core-v0.3.0) - 2026-08-16

### Added

- *(screens)* [**breaking**] the demo is an application, one screen per file
- *(core)* TabPages reports the tab switch it cannot repaint
- *(core)* [**breaking**] Screen - the screen is a component too
- *(core)* NoZone and ScrollZone, the two zones apps kept writing
- *(core)* [**breaking**] ask the widget who was pressed, not the index
- [**breaking**] drop the press/confirm latches now that Outcome carries them
- *(focus)* take the chrome and empty forms out of the focus order
- *(tabs)* add TabPages - the two-mode container behind a tab strip
- *(focus)* [**breaking**] step over zones that cannot hold the focus, and make sync_focus idempotent
- route the screen's focus with FocusChain
- [**breaking**] let update() say what became of the event

### Fixed

- *(focus)* end the edit on both sides when a form zone is left
- *(form)* keep focus and edit mode in step with a changing field set
- *(form)* repaint the whole form when its layout changes

### Other

- one public way to build a screen, and it is Screen
- *(tabs)* say who repaints a page arriving on a tab switch
- *(router)* write down how a screen's outcomes meet the router
- *(core)* keep rustdoc as quiet as it was
- *(widgets)* note the pre-first-frame page size; assert Marker's width contract

## [0.2.0](https://github.com/okislitsin/knurl/compare/knurl-core-v0.1.2...knurl-core-v0.2.0) - 2026-08-15

### Added

- draw focus as a band across the whole cursor row
- add a fill_band primitive for the focus row

### Fixed

- give Table, Radio and Tree leaves a cursor marker
- tidy up five inconsistencies around the edges
- say when a dialog's button row ran out of width
- give every scrolling widget the same dirty gate
- give the dialog message and button rows separate height thresholds
- keep the dialog button labels inside the box
- reject Router<_, 0> at compile time
- skip the dialog button row when the box is under two rows tall
- drop the hand-rolled scroll indicators in List and Form

## [0.1.2](https://github.com/okislitsin/knurl/compare/knurl-core-v0.1.1...knurl-core-v0.1.2) - 2026-07-09

### Fixed

- fix impl Component and FormField for Picker
