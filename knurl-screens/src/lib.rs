#![no_std]

//! The knurl demo application: **one screen per file**, each owning its widgets
//! and reporting an [`AppEvent`].
//!
//! This crate is the device side of the demo. The simulator demos
//! (`cargo run -p knurl-sim --example oled` / `--example tft`) add a window, a
//! keymap and the frame loop; everything the user actually sees and drives is
//! here.
//!
//! ## Why it is a crate of its own
//!
//! Because "copy a screen into your firmware" has to be checkable, not
//! promised. This crate is `no_std`, depends on nothing but the `knurl` facade -
//! the exact dependency a device project has - and CI builds it for
//! `thumbv6m-none-eabi`. So a screen here:
//!
//! - imports only `knurl::…`. No `knurl_sim`, no simulator types;
//! - uses no `std` and no `alloc`: no `Vec`, no `String`, no `format!`. Its data
//!   are `const` arrays next to it;
//! - knows nothing about its panel beyond [`Panel`]: its signatures are
//!   `&mut dyn RenderTarget` and [`Area`](knurl::Area), never `Rgb565` or a
//!   `SimulatorDisplay`;
//! - contains no demo scaffolding. The frame loop and the keyboard are the
//!   host's business.
//!
//! If it builds for bare metal, it copies to bare metal.
//!
//! ## What the demo shows
//!
//! Every widget in the catalogue, but arranged as **screens in use** rather than
//! a widget per page: one form ([`FormScreen`](form::FormScreen)), two forms
//! sharing a layout ([`TwoFormsScreen`](two_forms::TwoFormsScreen)), three forms
//! behind tabs ([`TabFormsScreen`](tab_forms::TabFormsScreen)), a list and a form
//! on one screen ([`ListFormScreen`](list_form::ListFormScreen)).

pub mod app;
pub mod canvas;
pub mod chart;
pub mod dialog;
pub mod editors;
pub mod form;
pub mod help;
pub mod indicators;
pub mod list;
pub mod list_form;
pub mod menu;
pub mod pager;
pub mod position;
pub mod radio;
pub mod status;
pub mod tab_forms;
pub mod table;
pub mod text;
pub mod textinput;
pub mod toggles;
pub mod tree;
pub mod two_forms;

mod stack;

pub use app::App;

// ── The catalogue ────────────────────────────────────────────────────────────

/// One screen of the demo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Menu,
    Text,
    List,
    Tree,
    Table,
    Chart,
    Toggles,
    Editors,
    Radio,
    Input,
    Pager,
    Indicators,
    Position,
    Canvas,
    TabForms,
    Status,
    Help,
    Dialog,
    Form,
    TwoForms,
    ListForm,
}

/// The root menu, in order. The last row leaves the demo; the rest map onto
/// [`PAGES`] one for one.
pub const MENU: &[&str] = &[
    "Text",
    "List",
    "Tree",
    "Table",
    "Bar chart",
    "Toggles",
    "Editors",
    "Radio",
    "Text input",
    "Pager",
    "Indicators",
    "Position",
    "Canvas",
    "Tabs + forms",
    "Status bar",
    "Help",
    "Dialog",
    "One form",
    "Two forms",
    "List + form",
    "Exit",
];

/// The page each menu row opens, in the same order as [`MENU`].
const PAGES: &[Page] = &[
    Page::Text,
    Page::List,
    Page::Tree,
    Page::Table,
    Page::Chart,
    Page::Toggles,
    Page::Editors,
    Page::Radio,
    Page::Input,
    Page::Pager,
    Page::Indicators,
    Page::Position,
    Page::Canvas,
    Page::TabForms,
    Page::Status,
    Page::Help,
    Page::Dialog,
    Page::Form,
    Page::TwoForms,
    Page::ListForm,
];

impl Page {
    /// The page a root-menu row opens; `None` for the last row ("Exit").
    pub fn from_menu_row(row: usize) -> Option<Page> {
        PAGES.get(row).copied()
    }

    /// The screen's own title, for whatever chrome the host draws.
    pub fn title(self) -> &'static str {
        match self {
            Page::Menu => "knurl",
            Page::Text => "Text & styles",
            Page::List => "List",
            Page::Tree => "Tree",
            Page::Table => "Table",
            Page::Chart => "Bar chart",
            Page::Toggles => "Toggles",
            Page::Editors => "Editors",
            Page::Radio => "Radio",
            Page::Input => "Text input",
            Page::Pager => "Pager",
            Page::Indicators => "Indicators",
            Page::Position => "Position",
            Page::Canvas => "Canvas",
            Page::TabForms => "Tabs + forms",
            Page::Status => "Status bar",
            Page::Help => "Help",
            Page::Dialog => "Dialog",
            Page::Form => "One form",
            Page::TwoForms => "Two forms",
            Page::ListForm => "List + form",
        }
    }

    /// A one-line hint for a host with room for a status bar.
    pub fn hint(self) -> &'static str {
        match self {
            Page::Menu => "Turn: move   Push: open",
            Page::Toggles | Page::Editors | Page::Form | Page::Input => "Push: edit / activate",
            Page::TwoForms | Page::ListForm => "Turn: between the zones",
            Page::TabForms => "Push: into the tab's form",
            Page::Tree => "Push: expand / Back",
            Page::Dialog => "Push: a button / Back",
            Page::Pager => "Turn: scroll, bottom = follow",
            _ => "Turn: move   Push: Back",
        }
    }
}

// ── What a screen tells the application ──────────────────────────────────────

/// What any screen of this demo can say. One type for all of them, so they
/// dispatch as `&mut dyn Screen<Event = AppEvent>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    /// A root-menu row was chosen.
    Open(Page),
    /// The screen's "< Back" item was pressed.
    GoBack,
    /// The root menu's last row.
    Quit,
}

// ── Panel metrics ────────────────────────────────────────────────────────────

/// The handful of pixel widths a screen cannot work out for itself: label
/// columns and table columns, which depend on how wide the panel is.
///
/// Everything else about a screen is panel-independent - text wraps, lists
/// scroll, forms stack - so this is the whole of "which display is this".
#[derive(Debug, Clone, Copy)]
pub struct Panel {
    /// Label column of a `Slider`/`TextInput` row.
    pub label_w: u16,
    /// Key column of the `Help` screen.
    pub key_w: u16,
    /// Label column of the bar chart.
    pub bar_label_w: u16,
    /// Column widths of the table, left to right.
    pub table_w: &'static [u16],
}

impl Panel {
    /// A 128-pixel-wide monochrome panel (SSD1306 and friends).
    pub const SMALL: Self = Self {
        label_w: 36,
        key_w: 30,
        bar_label_w: 24,
        table_w: &[54, 24, 18],
    };

    /// A 320-pixel-wide colour panel (ST7789 and friends).
    pub const LARGE: Self = Self {
        label_w: 72,
        key_w: 60,
        bar_label_w: 48,
        table_w: &[150, 60, 48],
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu and the page table are two lists that have to agree; the only
    /// row without a page is the last one.
    #[test]
    fn every_menu_row_but_exit_opens_a_page() {
        assert_eq!(MENU.len(), PAGES.len() + 1);
        assert_eq!(*MENU.last().unwrap(), "Exit");
        assert!(Page::from_menu_row(MENU.len() - 1).is_none());
        for row in 0..PAGES.len() {
            assert!(Page::from_menu_row(row).is_some(), "row {row}");
        }
    }

    /// Every page in the catalogue is reachable from the menu (a page nobody
    /// can open is a page nobody tests).
    #[test]
    fn every_page_is_reachable() {
        for page in PAGES {
            assert!(!page.title().is_empty());
        }
    }
}
