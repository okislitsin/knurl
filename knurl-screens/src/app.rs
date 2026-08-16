//! The application: a [`Router`], one screen per page, and the two places they
//! meet.
//!
//! There is no per-page logic here and nowhere for any to accumulate:
//! [`App::screen`] says *which* screen is current and hands back a reference,
//! [`App::update`] turns whatever that screen reported into navigation. A new
//! page means a new file and two lines here.

use knurl::{Area, LinesModel, Msg, RenderTarget, Router, Screen};

use crate::{
    AppEvent, Page, Panel, canvas::CanvasScreen, chart::ChartScreen, dialog::DialogScreen,
    editors::EditorsScreen, form::FormScreen, help::HelpScreen, indicators::IndicatorsScreen,
    list::ListScreen, list_form::ListFormScreen, menu::MenuScreen, pager::PagerScreen,
    position::PositionScreen, radio::RadioScreen, status::StatusScreen, tab_forms::TabFormsScreen,
    table::TableScreen, text::TextScreen, textinput::TextInputScreen, toggles::TogglesScreen,
    tree::TreeScreen, two_forms::TwoFormsScreen,
};

/// How deep the demo ever nests: the menu plus one page.
const DEPTH: usize = 4;

/// The whole demo. `M` is where the [`PagerScreen`] gets its lines - a `const`
/// array on a device, a growing ring buffer in the TFT demo.
pub struct App<'a, M: LinesModel + ?Sized> {
    router: Router<Page, DEPTH>,
    quit: bool,

    menu: MenuScreen,
    text: TextScreen,
    list: ListScreen,
    tree: TreeScreen,
    table: TableScreen,
    chart: ChartScreen,
    toggles: TogglesScreen,
    editors: EditorsScreen,
    radio: RadioScreen,
    input: TextInputScreen,
    pager: PagerScreen<'a, M>,
    indicators: IndicatorsScreen,
    position: PositionScreen,
    canvas: CanvasScreen,
    tab_forms: TabFormsScreen,
    status: StatusScreen,
    help: HelpScreen,
    dialog: DialogScreen,
    form: FormScreen,
    two_forms: TwoFormsScreen,
    list_form: ListFormScreen,
}

impl<'a, M: LinesModel + ?Sized> App<'a, M> {
    pub fn new(panel: Panel, lines: &'a M) -> Self {
        let mut app = Self {
            router: Router::new(Page::Menu),
            quit: false,
            menu: MenuScreen::new(),
            text: TextScreen::new(),
            list: ListScreen::new(),
            tree: TreeScreen::new(),
            table: TableScreen::new(panel),
            chart: ChartScreen::new(panel),
            toggles: TogglesScreen::new(),
            editors: EditorsScreen::new(panel),
            radio: RadioScreen::new(),
            input: TextInputScreen::new(panel),
            pager: PagerScreen::new(lines),
            indicators: IndicatorsScreen::new(),
            position: PositionScreen::new(),
            canvas: CanvasScreen::new(),
            tab_forms: TabFormsScreen::new(panel),
            status: StatusScreen::new(),
            help: HelpScreen::new(panel),
            dialog: DialogScreen::new(),
            form: FormScreen::new(),
            two_forms: TwoFormsScreen::new(panel),
            list_form: ListFormScreen::new(),
        };
        app.screen().enter();
        app
    }

    /// Pins the pager to the newest line - for a log that grows while it is
    /// being watched.
    pub fn with_log_follow(mut self, on: bool) -> Self {
        self.pager = self.pager.with_follow(on);
        self
    }

    /// The dispatcher: which screen is current. The only `match` on a page in
    /// the whole application, and it contains nothing but references.
    fn screen(&mut self) -> &mut dyn Screen<Event = AppEvent> {
        match self.router.current() {
            Page::Menu => &mut self.menu,
            Page::Text => &mut self.text,
            Page::List => &mut self.list,
            Page::Tree => &mut self.tree,
            Page::Table => &mut self.table,
            Page::Chart => &mut self.chart,
            Page::Toggles => &mut self.toggles,
            Page::Editors => &mut self.editors,
            Page::Radio => &mut self.radio,
            Page::Input => &mut self.input,
            Page::Pager => &mut self.pager,
            Page::Indicators => &mut self.indicators,
            Page::Position => &mut self.position,
            Page::Canvas => &mut self.canvas,
            Page::TabForms => &mut self.tab_forms,
            Page::Status => &mut self.status,
            Page::Help => &mut self.help,
            Page::Dialog => &mut self.dialog,
            Page::Form => &mut self.form,
            Page::TwoForms => &mut self.two_forms,
            Page::ListForm => &mut self.list_form,
        }
    }

    /// Routes one event and applies whatever the screen made of it. This is the
    /// only place navigation happens.
    pub fn update(&mut self, msg: &Msg) {
        let Some(event) = self.screen().update(msg) else {
            return;
        };
        match event {
            AppEvent::Open(page) => {
                self.router.push(page);
            }
            // "Back" at the root is the way out of the demo - the encoder has
            // no other one.
            AppEvent::GoBack => self.quit = !self.router.pop(),
            AppEvent::Quit => {
                self.quit = true;
                return;
            }
        }
        // The screen arriving places its cursor and repaints over its
        // predecessor. (The one leaving keeps its state for when it comes back.)
        self.screen().enter();
    }

    /// Advances the current screen's animation, past the focus chain. Reports
    /// whether the frame is worth painting.
    pub fn tick(&mut self) -> bool {
        self.screen().tick()
    }

    /// Paints the current screen into `area`. Whatever chrome surrounds it -
    /// a title, a status bar - belongs to the host.
    pub fn view(&mut self, target: &mut dyn RenderTarget, area: Area) {
        self.screen().view(target, area);
    }

    pub fn page(&self) -> Page {
        self.router.current()
    }

    /// The current screen's title, for the host's chrome.
    pub fn title(&self) -> &'static str {
        self.page().title()
    }

    /// A one-line hint about the current screen, for a host with room for it.
    pub fn hint(&self) -> &'static str {
        self.page().hint()
    }

    /// Whether the user has left the root menu through its last row (or
    /// through a "< Back" with nowhere left to go).
    pub fn quit(&self) -> bool {
        self.quit
    }
}
