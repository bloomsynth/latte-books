//! A Link or Select field: a text input with a dropdown of matching options, typed into
//! directly.
//!
//! Behaviour follows Books' `src/components/Controls/AutoComplete.vue`, `Link.vue` and
//! `Select.vue` @ v0.36.0: the list opens on focus, on click and while typing, ↑/↓ move the
//! highlight, Enter picks, Tab, Esc and a click elsewhere close it.
//!
//! Differences from Books: matching is a case-insensitive substring match, not fuzzy, and
//! there is no "Create" entry (Books opens a quick-entry form for a new linked document).

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// An option was picked: its value.
pub struct LinkPicked(pub SharedString);

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
    _subscription: Subscription,
}

impl EventEmitter<LinkPicked> for LinkInput {}

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
            _subscription: subscription,
        }
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let dropdown = (self.open && !self.entries.is_empty()).then(|| {
            let rows = self.entries.iter().enumerate().map(|(i, option)| {
                let label = self.options[*option].label.clone();
                div()
                    .id(i)
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .when(i == self.highlighted, |row| row.bg(theme.list_active))
                    .hover(|row| row.bg(theme.list_hover))
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
                    div()
                        .id("link-dropdown")
                        .occlude()
                        .mt_1()
                        .min_w(px(220.))
                        .max_h(px(260.))
                        .overflow_y_scroll()
                        .p_1()
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .border_1()
                        .border_color(theme.border)
                        .rounded_md()
                        .shadow_md()
                        .children(rows),
                ),
            )
            .with_priority(1)
        });

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
            .child(Input::new(&self.input).small())
            .children(dropdown)
    }
}
