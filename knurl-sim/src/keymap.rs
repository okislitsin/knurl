//! Keyboard → [`Msg`] mapping.
//!
//! The mapping is a **configurable struct**, not a hardcoded match. The default
//! ([`Keymap::encoder`]) mirrors the target hardware - a rotary encoder with one
//! push button: rotation is `Up`/`Down`, the button is `Select`, nothing else.
//! [`Keymap::full`] is an opt-in keyboard layout for desktop prototyping only -
//! it is **not** the default and is not used by any example. See [`Keymap`].

use embedded_graphics_simulator::sdl2::Keycode;
use knurl_core::Msg;

/// Translates SDL key presses into [`Msg`] values.
///
/// Each directional/action role is an optional [`Keycode`]; setting a field to
/// `None` disables that role. This is what lets the default config shrink to the
/// encoder model - [`up`](Keymap::up)/[`down`](Keymap::down)/
/// [`select`](Keymap::select) only, the rest cleared (see [`Keymap::encoder`]).
///
/// When [`chars`](Keymap::chars) is set, any printable ASCII key that does not
/// match a role maps to [`Msg::Char`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keymap {
    pub up: Option<Keycode>,
    pub down: Option<Keycode>,
    pub left: Option<Keycode>,
    pub right: Option<Keycode>,
    pub select: Option<Keycode>,
    pub back: Option<Keycode>,
    /// Map unmatched printable ASCII keys to [`Msg::Char`].
    pub chars: bool,
}

impl Keymap {
    /// The **default** model: a rotary encoder with one push button.
    ///
    /// - `↑` / `↓` → [`Msg::Up`] / [`Msg::Down`] (encoder rotation)
    /// - `Space` → [`Msg::Select`] (the push button)
    ///
    /// `Left`/`Right`/`Back`/`Char` are all disabled - the device has no such
    /// inputs. "Back" is expected to be an in-app menu item reached with
    /// Up/Down/Select, not a key.
    pub const fn encoder() -> Self {
        Self {
            up: Some(Keycode::Up),
            down: Some(Keycode::Down),
            left: None,
            right: None,
            select: Some(Keycode::Space),
            back: None,
            chars: false,
        }
    }

    /// Opt-in **keyboard** layout for desktop prototyping - *not* the default
    /// and not the hardware model. Adds keys the encoder doesn't have:
    ///
    /// - `Up`/`Down`/`Left`/`Right` → [`Msg::Up`]/[`Msg::Down`]/[`Msg::Left`]/[`Msg::Right`]
    /// - `Return` → [`Msg::Select`]
    /// - `Escape` → [`Msg::Back`]
    /// - printable ASCII → [`Msg::Char`]
    pub const fn full() -> Self {
        Self {
            up: Some(Keycode::Up),
            down: Some(Keycode::Down),
            left: Some(Keycode::Left),
            right: Some(Keycode::Right),
            select: Some(Keycode::Return),
            back: Some(Keycode::Escape),
            chars: true,
        }
    }

    /// Translate a pressed key into a [`Msg`], or `None` if it is unmapped.
    ///
    /// Role bindings take priority over the printable-char fallback, so a key
    /// bound to a role (e.g. Return → `Select`) never also emits `Char`.
    pub fn map(&self, key: Keycode) -> Option<Msg> {
        if self.up == Some(key) {
            return Some(Msg::Up);
        }
        if self.down == Some(key) {
            return Some(Msg::Down);
        }
        if self.left == Some(key) {
            return Some(Msg::Left);
        }
        if self.right == Some(key) {
            return Some(Msg::Right);
        }
        if self.select == Some(key) {
            return Some(Msg::Select);
        }
        if self.back == Some(key) {
            return Some(Msg::Back);
        }
        if self.chars {
            // SDL keycodes for printable ASCII equal their character value.
            let code = i32::from(key);
            if (0x20..=0x7E).contains(&code) {
                return Some(Msg::Char(code as u8 as char));
            }
        }
        None
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::encoder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_encoder() {
        assert_eq!(Keymap::default(), Keymap::encoder());
    }

    #[test]
    fn encoder_maps_rotation_and_button() {
        let k = Keymap::encoder();
        // Rotation.
        assert_eq!(k.map(Keycode::Up), Some(Msg::Up));
        assert_eq!(k.map(Keycode::Down), Some(Msg::Down));
        // Push button is Space (not Return).
        assert_eq!(k.map(Keycode::Space), Some(Msg::Select));
        assert_eq!(k.map(Keycode::Return), None);
    }

    #[test]
    fn encoder_has_no_back_arrows_or_chars() {
        let k = Keymap::encoder();
        // The device has no Left/Right, no Back, no character entry.
        assert_eq!(k.map(Keycode::Left), None);
        assert_eq!(k.map(Keycode::Right), None);
        assert_eq!(k.map(Keycode::Escape), None);
        assert_eq!(k.map(Keycode::A), None);
        assert_eq!(k.map(Keycode::Num5), None);
    }

    #[test]
    fn full_maps_navigation_and_actions() {
        let k = Keymap::full();
        assert_eq!(k.map(Keycode::Up), Some(Msg::Up));
        assert_eq!(k.map(Keycode::Down), Some(Msg::Down));
        assert_eq!(k.map(Keycode::Left), Some(Msg::Left));
        assert_eq!(k.map(Keycode::Right), Some(Msg::Right));
        assert_eq!(k.map(Keycode::Return), Some(Msg::Select));
        assert_eq!(k.map(Keycode::Escape), Some(Msg::Back));
    }

    #[test]
    fn full_maps_printable_chars() {
        let k = Keymap::full();
        assert_eq!(k.map(Keycode::A), Some(Msg::Char('a')));
        assert_eq!(k.map(Keycode::Num5), Some(Msg::Char('5')));
        assert_eq!(k.map(Keycode::Space), Some(Msg::Char(' ')));
    }

    #[test]
    fn role_binding_wins_over_char_fallback() {
        // Return is bound to Select, so it must not also emit a Char even though
        // it is a "printable" carriage-return-ish key in some keymaps.
        let k = Keymap::full();
        assert_eq!(k.map(Keycode::Return), Some(Msg::Select));
    }

    #[test]
    fn unmapped_key_returns_none() {
        let k = Keymap::full();
        assert_eq!(k.map(Keycode::F1), None);
    }
}
