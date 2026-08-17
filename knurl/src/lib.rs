#![no_std]

// Facade: re-export everything from core so downstream users only need
// `knurl` in their Cargo.toml — every widget plus the `Router`/`Nav`
// navigation backbone.
pub use knurl_core::{
    Align, Area, BarChart, BarChartModel, BorderStyle, Bordered, Button, Canvas, Checkbox,
    Component, Constraint, Counter, Dialog, Entry, FocusChain, FocusZone, Form, FormField,
    FormZone, HStack, Help, Label, LineGauge, LinesModel, List, ListModel, Marker, Msg, Nav,
    NoZone, Outcome, Padded, Padding, Pager, Paginator, Picker, PickerItem, ProgressBar, Radio,
    RenderTarget, Router, Screen, ScreenState, ScrollZone, Scrollbar, Separator, Slider, Spacer,
    Spinner, SpinnerStyle, StatusBar, Style, TabPages, TabZone, Table, TableModel, Tabs, TextInput,
    Title, Toggle, Tree, TreeItem, TreeModel, VStack, bitmap_runs,
};

/// How to write a widget of your own: the `Component` contract, what a frame
/// costs, and the traps that are already known.
pub use knurl_core::custom_widget;

#[cfg(feature = "graphics")]
pub use knurl_graphics as graphics;
