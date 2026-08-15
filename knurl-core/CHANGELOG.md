# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
