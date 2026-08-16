# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
