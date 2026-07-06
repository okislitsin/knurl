//! Screen-history navigation for multi-screen apps.
//!
//! [`Router`] is a fixed-depth stack of screen identifiers - it tracks *which*
//! screen is current and the trail of screens behind it; it does **not** render.
//! Rendering and per-screen state live in the application.
//!
//! ## The encoder navigation pattern
//!
//! On encoder hardware the only inputs are Up / Down / Select - there is no
//! dedicated Back button, so "Back" is a focusable menu item (see the
//! [`Msg`](crate::Msg) model). The app loop is a `match` on the current screen:
//!
//! ```
//! use knurl_core::Router;
//!
//! #[derive(Debug, Clone, Copy, PartialEq)]
//! enum Screen { Menu, Settings, About }
//!
//! let mut router = Router::<Screen, 8>::new(Screen::Menu);
//!
//! // Selecting "Settings" on the menu opens it on top of the history:
//! router.push(Screen::Settings);
//! assert_eq!(router.current(), Screen::Settings);
//!
//! // A "Back" item on the sub-screen returns to the menu:
//! let went_back = router.pop();
//! assert!(went_back);
//! assert_eq!(router.current(), Screen::Menu);
//!
//! // "Back" while at the root is the app's signal to exit:
//! if router.at_root() {
//!     // quit, or ignore - application's choice.
//! }
//! ```
//!
//! Each render, the app matches on [`current`](Router::current) to draw the
//! right screen. A "Back" item calls [`pop`](Router::pop); when
//! [`at_root`](Router::at_root) reports the stack is already at the bottom, the
//! app treats a Back as "exit". All operations saturate gracefully - popping the
//! root and pushing past `DEPTH` are bounded no-ops, never panics.

// ── Nav ─────────────────────────────────────────────────────────────────────

/// A navigation intent, returned by a screen for the application to apply to a
/// [`Router`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav<Id> {
    /// Open a new screen on top, keeping the current one in history.
    Push(Id),
    /// Return to the previous screen.
    Pop,
    /// Replace the current screen without growing the history.
    Replace(Id),
}

// ── Router ──────────────────────────────────────────────────────────────────

/// A fixed-depth history stack of screen identifiers.
///
/// The stack holds at most `DEPTH` screens (`DEPTH >= 1`); the current screen is
/// the top of the stack. `Id` is the application's screen identifier - typically
/// a `Copy` enum. Screen state and `update`/`view` live in the application (a
/// `match id` dispatcher); the router only tracks history - no heap allocation.
#[derive(Debug)]
pub struct Router<Id, const DEPTH: usize> {
    stack: [Id; DEPTH],
    len: usize,
}

impl<Id: Copy, const DEPTH: usize> Router<Id, DEPTH> {
    /// Creates a router with `root` as the bottom screen. The root cannot be
    /// popped (`pop` never removes it).
    pub fn new(root: Id) -> Self {
        Self { stack: [root; DEPTH], len: 1 }
    }

    /// The current (top) screen.
    pub fn current(&self) -> Id {
        self.stack[self.len - 1]
    }

    /// The stack depth (always `>= 1`).
    pub fn depth(&self) -> usize {
        self.len
    }

    /// Whether the current screen is the root (the stack cannot be popped any
    /// further). A "Back" here is the app's cue to exit.
    pub fn at_root(&self) -> bool {
        self.len <= 1
    }

    /// Whether [`pop`](Router::pop) would return to a previous screen - the
    /// inverse of [`at_root`](Router::at_root). Useful to decide whether a
    /// "Back" item should pop or signal exit.
    pub fn can_pop(&self) -> bool {
        self.len > 1
    }

    /// Pushes a screen on top. Returns `false` when the stack is full (`DEPTH`
    /// reached).
    pub fn push(&mut self, id: Id) -> bool {
        if self.len >= DEPTH {
            return false;
        }
        self.stack[self.len] = id;
        self.len += 1;
        true
    }

    /// Returns to the previous screen. Returns `false` when already at the root.
    pub fn pop(&mut self) -> bool {
        if self.len <= 1 {
            return false;
        }
        self.len -= 1;
        true
    }

    /// Replaces the current screen without changing the depth.
    pub fn replace(&mut self, id: Id) {
        self.stack[self.len - 1] = id;
    }

    /// Applies a navigation intent.
    pub fn apply(&mut self, nav: Nav<Id>) {
        match nav {
            Nav::Push(id) => {
                self.push(id);
            }
            Nav::Pop => {
                self.pop();
            }
            Nav::Replace(id) => self.replace(id),
        }
    }

    /// Resets the history to a single `root` screen.
    pub fn reset(&mut self, root: Id) {
        self.stack[0] = root;
        self.len = 1;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum S {
        Menu,
        Settings,
        Sub,
    }

    #[test]
    fn router_starts_at_root() {
        let r = Router::<S, 4>::new(S::Menu);
        assert_eq!(r.current(), S::Menu);
        assert_eq!(r.depth(), 1);
    }

    #[test]
    fn router_at_root_and_can_pop() {
        let mut r = Router::<S, 4>::new(S::Menu);
        assert!(r.at_root());
        assert!(!r.can_pop());
        r.push(S::Settings);
        assert!(!r.at_root());
        assert!(r.can_pop());
        r.pop();
        assert!(r.at_root());
        assert!(!r.can_pop());
    }

    #[test]
    fn router_push_pop() {
        let mut r = Router::<S, 4>::new(S::Menu);
        assert!(r.push(S::Settings));
        assert_eq!(r.current(), S::Settings);
        assert_eq!(r.depth(), 2);
        assert!(r.push(S::Sub));
        assert_eq!(r.current(), S::Sub);
        assert_eq!(r.depth(), 3);

        assert!(r.pop());
        assert_eq!(r.current(), S::Settings);
        assert!(r.pop());
        assert_eq!(r.current(), S::Menu);
        assert!(!r.pop()); // at root
        assert_eq!(r.current(), S::Menu);
        assert_eq!(r.depth(), 1);
    }

    #[test]
    fn router_push_full_returns_false() {
        let mut r = Router::<S, 2>::new(S::Menu);
        assert!(r.push(S::Settings));
        assert!(!r.push(S::Sub));
        assert_eq!(r.current(), S::Settings);
        assert_eq!(r.depth(), 2);
    }

    #[test]
    fn router_replace_keeps_depth() {
        let mut r = Router::<S, 4>::new(S::Menu);
        r.push(S::Settings);
        r.replace(S::Sub);
        assert_eq!(r.current(), S::Sub);
        assert_eq!(r.depth(), 2);
    }

    #[test]
    fn router_apply_nav() {
        let mut r = Router::<S, 4>::new(S::Menu);
        r.apply(Nav::Push(S::Settings));
        assert_eq!(r.current(), S::Settings);
        r.apply(Nav::Replace(S::Sub));
        assert_eq!(r.current(), S::Sub);
        assert_eq!(r.depth(), 2);
        r.apply(Nav::Pop);
        assert_eq!(r.current(), S::Menu);
        assert_eq!(r.depth(), 1);
    }

    #[test]
    fn router_reset() {
        let mut r = Router::<S, 4>::new(S::Menu);
        r.push(S::Settings);
        r.push(S::Sub);
        r.reset(S::Menu);
        assert_eq!(r.depth(), 1);
        assert_eq!(r.current(), S::Menu);
    }
}
