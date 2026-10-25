//! A Link or Select field: a text input with a dropdown of matching options, typed into
//! directly.
//!
//! Behaviour follows Books' `src/components/Controls/AutoComplete.vue`, `Link.vue` and
//! `Select.vue` @ v0.36.0: the list opens on focus, on click and while typing, ↑/↓ move the
//! highlight, Enter picks, Tab, Esc and a click elsewhere close it.
//!
//! With a value, a Link field shows Books' chevron to open the linked document; otherwise
//! the up-and-down selector.
//!
//! Differences from Books: matching is a case-insensitive substring match, not fuzzy, and
//! there is no "Create" entry (Books opens a quick-entry form for a new linked document).

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, Text as _, gray};
use crate::widgets::{control_frame, popover_panel, selector_icon};

/// An option was picked: its value.
pub struct LinkPicked(pub SharedString);

/// The chevron of a Link field was clicked: open the linked document, by its name.
pub struct LinkOpened(pub SharedString);

/// An option: its value and what is shown for it.
#[derive(Clone)]
pub struct LinkOption {
    pub value: SharedString,
    pub label: SharedString,
}

pub struct LinkInput {
    input: Entity<InputState>,
    options: Vec<LinkOption>,
    entries: Vec<usize>,
    highlighted: usize,
    open: bool,
    value: Option<SharedString>,
    /// Drawn in Books' bordered frame (a form field), or bare (a table cell).
    bordered: bool,
    /// A Link field, whose value can be opened.
    linkable: bool,
    /// Required and empty: the selector is red (`showMandatory`).
    mandatory: bool,
    /// `size="small"`: `px-2 py-1`, as report filters are.
    small: bool,
    _subscription: Subscription,
}

impl EventEmitter<LinkPicked> for LinkInput {}
impl EventEmitter<LinkOpened> for LinkInput {}

impl LinkInput {
    pub fn new(
        options: Vec<LinkOption>,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        Self {
            input,
            options,
            entries: Vec::new(),
            highlighted: 0,
            open: false,
            value: None,
            bordered: true,
            linkable: false,
            mandatory: false,
            small: false,
            _subscription: subscription,
        }
    }

    /// As a table cell: no frame.
    pub fn bare(mut self) -> Self {
        self.bordered = false;
        self
    }

    /// The small size, as report filters are.
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    /// A Link field, which shows a chevron to open its value.
    pub fn linkable(mut self, linkable: bool) -> Self {
        self.linkable = linkable;
        self
    }

    pub fn set_mandatory(&mut self, mandatory: bool) {
        self.mandatory = mandatory;
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.value.as_ref()
    }

    pub fn set_options(&mut self, options: Vec<LinkOption>, cx: &mut Context<Self>) {
        self.options = options;
        self.refilter(cx);
    }

    fn label_of(&self, value: &str) -> SharedString {
        self.options
            .iter()
            .find(|option| option.value == value)
            .map_or_else(|| SharedString::from(value.to_owned()), |o| o.label.clone())
    }

    /// Sets the value without emitting [`LinkPicked`], as a field set by another field
    /// would. Does nothing while the input has focus, so typing isn't overwritten.
    pub fn set_value(
        &mut self,
        value: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.value == value {
            return;
        }
        let label = value
            .as_deref()
            .map(|v| self.label_of(v))
            .unwrap_or_default();
        self.input
            .update(cx, |input, cx| input.set_value(label, window, cx));
        self.value = value;
        self.open = false;
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }

    fn query(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }

    fn refilter(&mut self, cx: &App) {
        let query = self.query(cx);
        let needle = query.trim().to_lowercase();
        // The text showing the current value matches everything, so the whole list opens.
        let showing_value = self
            .value
            .as_deref()
            .is_some_and(|v| self.label_of(v).to_lowercase() == needle);
        self.entries = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, option)| {
                showing_value
                    || option.label.to_lowercase().contains(&needle)
                    || option.value.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        self.highlighted = 0;
    }

    fn on_input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Focus | InputEvent::Change => {
                self.refilter(cx);
                self.open = true;
            }
            InputEvent::Blur => {
                self.open = false;
                // Typed text only sticks if it names an option; otherwise restore the value.
                let query = self.query(cx);
                let typed = query.trim();
                let found = self
                    .options
                    .iter()
                    .find(|o| {
                        o.value.eq_ignore_ascii_case(typed) || o.label.eq_ignore_ascii_case(typed)
                    })
                    .map(|o| o.value.clone());
                match found {
                    Some(value) if self.value.as_ref() != Some(&value) => {
                        self.set_value(Some(value.clone()), window, cx);
                        cx.emit(LinkPicked(value));
                    }
                    Some(_) => {}
                    None if typed.is_empty() && self.value.is_some() => {
                        self.value = None;
                        cx.emit(LinkPicked(SharedString::default()));
                    }
                    None => {
                        let restore = self
                            .value
                            .as_deref()
                            .map(|v| self.label_of(v))
                            .unwrap_or_default();
                        self.input
                            .update(cx, |input, cx| input.set_value(restore, window, cx));
                    }
                }
            }
            InputEvent::PressEnter { .. } => {}
        }
        cx.notify();
    }

    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(option) = self.entries.get(index).and_then(|i| self.options.get(*i)) else {
            return;
        };
        let value = option.value.clone();
        self.value = None;
        self.set_value(Some(value.clone()), window, cx);
        cx.emit(LinkPicked(value));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.entries.len();
        match event.keystroke.key.as_str() {
            "down" => {
                if self.open && count > 0 {
                    self.highlighted = (self.highlighted + 1) % count;
                } else {
                    self.refilter(cx);
                    self.open = true;
                }
            }
            "up" if self.open && count > 0 => {
                self.highlighted = (self.highlighted + count - 1) % count;
            }
            "enter" if self.open && count > 0 => self.choose(self.highlighted, window, cx),
            "escape" if self.open => self.open = false,
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for LinkInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.is_focused(window, cx);
        // `Dropdown.vue`: items `p-2 rounded-md`, the highlighted one gray-100.
        let dropdown = (self.open && !self.entries.is_empty()).then(|| {
            let rows = self.entries.iter().enumerate().map(|(i, option)| {
                let label = self.options[*option].label.clone();
                div()
                    .id(i)
                    .p_2()
                    .when(i > 0, |d| d.mt_1())
                    .rounded(books::RADIUS_MD)
                    .truncate()
                    .cursor_pointer()
                    .when(i == self.highlighted, |row| row.bg(gray(100)))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.highlighted != i {
                            this.highlighted = i;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.choose(i, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child(label)
            });
            deferred(
                anchored().snap_to_window().child(
                    popover_panel()
                        .id("link-dropdown")
                        .occlude()
                        .mt_2()
                        .min_w(px(160.))
                        .max_h(px(256.))
                        .overflow_y_scroll()
                        .p_1()
                        .text_12()
                        .children(rows),
                ),
            )
            .with_priority(1)
        });

        let link = self
            .linkable
            .then(|| self.value.clone())
            .flatten()
            .map(|value| {
                div()
                    .id("open-link")
                    .p(px(2.))
                    .rounded(books::RADIUS)
                    .cursor_pointer()
                    .child(books::icon("chevron-right", px(16.), gray(600)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(LinkOpened(value.clone()))))
            });
        let has_link = link.is_some();

        let input = Input::new(&self.input)
            .appearance(false)
            .h(px(32.))
            .text_13()
            .map(|i| if self.bordered { i.px_3() } else { i.px_2() });
        let frame = if self.bordered {
            control_frame(false, focused).when(self.small, |d| d.min_h(px(30.)))
        } else {
            div()
                .h_flex()
                .w_full()
                .rounded(books::RADIUS)
                .text_13()
                .text_color(gray(900))
                .when(focused, |d| d.bg(gray(100)))
        };

        div()
            .v_flex()
            .w_full()
            .capture_key_down(cx.listener(Self::on_key_down))
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if !this.open {
                    this.refilter(cx);
                    this.open = true;
                    cx.notify();
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if this.open {
                    this.open = false;
                    cx.notify();
                }
            }))
            .child(
                frame
                    .pr_2()
                    .child(div().flex_1().min_w_0().child(input))
                    .children(link)
                    .when(!has_link, |d| d.child(selector_icon(self.mandatory))),
            )
            .children(dropdown)
    }
}
