//! A [`Tree`] and a way out. `Select` folds a parent or picks a leaf - which is
//! why this screen has to tell an activation from the tree apart from a press
//! of its own button, and does it by asking the button.

use knurl::{
    Area, Button, Component,
    Constraint::{Fill, Length},
    FocusChain, FocusZone, Msg, Outcome, RenderTarget, Screen, ScreenState, Tree, TreeItem, VStack,
};

use crate::AppEvent;

const ITEMS: &[TreeItem] = &[
    TreeItem::new("project", 0),
    TreeItem::new("src", 1),
    TreeItem::new("main", 2),
    TreeItem::new("lib", 2),
    TreeItem::new("docs", 1),
    TreeItem::new("guide", 2),
    TreeItem::new("readme", 1),
];

pub struct TreeScreen {
    state: ScreenState,
    tree: Tree<'static>,
    back: Button<'static>,
}

impl TreeScreen {
    pub fn new() -> Self {
        Self {
            state: ScreenState::new(),
            tree: Tree::new(ITEMS),
            back: Button::new("< Back"),
        }
    }
}

impl Default for TreeScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for TreeScreen {
    type Event = AppEvent;

    fn state(&mut self) -> &mut ScreenState {
        &mut self.state
    }

    fn zones(&mut self, f: &mut dyn FnMut(&mut FocusChain, &mut [&mut dyn FocusZone])) {
        let Self { state, tree, back } = self;
        f(state.chain(), &mut [tree, back]);
    }

    /// A leaf chosen in the tree is an `Activated` too - but it is not this
    /// button's, so the screen stays put.
    fn on_outcome(&mut self, _msg: &Msg, _outcome: Outcome) -> Option<AppEvent> {
        self.back.take_pressed().then_some(AppEvent::GoBack)
    }

    fn draw(&mut self, target: &mut dyn RenderTarget, area: Area) {
        let lh = target.line_height().max(1);
        let [body, foot] = VStack::split(area, &[Fill(1), Length(lh)]);
        self.tree.view(target, body);
        self.back.view(target, foot);
    }
}
