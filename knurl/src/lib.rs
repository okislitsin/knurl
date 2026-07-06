#![no_std]

// Facade: re-export everything from core so downstream users only need
// `knurl` in their Cargo.toml — every widget plus the `Router`/`Nav`
// navigation backbone.
pub use knurl_core::{
    Align, Area, BarChart, BarChartModel, BorderStyle, Bordered, Button, Checkbox, Component,
    Constraint, Counter, Dialog, Form, FormField, HStack, Help, Label, LineGauge, LinesModel, List,
    ListModel, Marker, Msg, Nav, Padded, Padding, Pager, Paginator, Picker, ProgressBar, Radio,
    RenderTarget, Router, Scrollbar, Separator, Slider, Spacer, Spinner, SpinnerStyle, StatusBar,
    Style, Table, TableModel, Tabs, TextInput, Title, Toggle, Tree, TreeItem, TreeModel, VStack,
};

#[cfg(feature = "graphics")]
pub use knurl_graphics as graphics;
