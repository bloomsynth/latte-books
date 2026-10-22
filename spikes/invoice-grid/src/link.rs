//! A Link field: a text input with a dropdown of matching records, typed into directly.
//!
//! Behaviour follows Books' `src/components/Controls/AutoComplete.vue` and `Link.vue` @ v0.36.0:
//! the list opens on focus and while typing, ↑/↓ move the highlight, Enter picks,
//! Tab and Esc close, and a "Create" entry is offered when nothing matches exactly.
//! Books uses fuzzy matching; this spike uses a case-insensitive substring match.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub enum LinkEvent {
    /// An existing option was picked.
    Selected(SharedString),
    /// The user chose "Create" for text that matches no option.
    Create(SharedString),
}

enum Entry {
    Option(SharedString),
    Create(SharedString),
}

pub struct LinkInput {
    input: Entity<InputState>,
    options: Vec<SharedString>,
    entries: Vec<Entry>,
    highlighted: usize,
    open: bool,
    value: Option<SharedString>,
    allow_create: bool,
    _subscription: Subscription,
}

impl EventEmitter<LinkEvent> for LinkInput {}

impl LinkInput {
    pub fn new(
        options: Vec<SharedString>,
        placeholder: &'static str,
        allow_create: bool,
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
            allow_create,
            _subscription: subscription,
        }
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.value.as_ref()
    }

    pub fn set_options(&mut self, options: Vec<SharedString>, cx: &mut Context<Self>) {
        self.options = options;
        self.refilter(cx);
    }

    /// Sets the value without emitting [`LinkEvent`], as a field set by another field would.
    pub fn set_value(&mut self, value: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        self.value = Some(value);
        self.open = false;
        cx.notify();
    }

    /// Focuses the input and opens the list. Opening here doesn't depend on
    /// `InputEvent::Focus`, which a just-created input can miss: its focus listener
    /// is only registered once the current update finishes.
    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let input = this.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        this.update(cx, |link, cx| {
            link.refilter(cx);
            link.open = true;
            cx.notify();
        });
    }

    #[cfg(test)]
    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }

    /// Whether the dropdown is showing, and the entry it highlights.
    #[cfg(test)]
    pub fn dropdown(&self) -> Option<SharedString> {
        self.open.then(|| match self.entries.get(self.highlighted) {
            Some(Entry::Option(name) | Entry::Create(name)) => name.clone(),
            None => SharedString::default(),
        })
    }

    #[cfg(test)]
    pub fn text(&self, cx: &App) -> SharedString {
        self.query(cx)
    }

    fn query(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }

    fn refilter(&mut self, cx: &App) {
        let query = self.query(cx);
        let needle = query.trim().to_lowercase();
        self.entries = self
            .options
            .iter()
            .filter(|option| option.to_lowercase().contains(&needle))
            .cloned()
            .map(Entry::Option)
            .collect();
        let exact = self
            .options
            .iter()
            .any(|option| option.to_lowercase() == needle);
        if self.allow_create && !needle.is_empty() && !exact {
            self.entries
                .push(Entry::Create(query.trim().to_string().into()));
        }
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
            InputEvent::Focus => {
                self.refilter(cx);
                self.open = true;
            }
            InputEvent::Change => {
                self.refilter(cx);
                self.open = true;
            }
            InputEvent::Blur => {
                self.open = false;
                // Typed text only sticks if it names an option; otherwise restore the value.
                let query = self.query(cx);
                let typed = self
                    .options
                    .iter()
                    .find(|option| option.eq_ignore_ascii_case(query.trim()))
                    .cloned();
                match typed {
                    Some(option) if self.value.as_ref() != Some(&option) => {
                        self.set_value(option.clone(), window, cx);
                        cx.emit(LinkEvent::Selected(option));
                    }
                    Some(_) => {}
                    None => {
                        let restore = self.value.clone().unwrap_or_default();
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
        let event = match self.entries.get(index) {
            Some(Entry::Option(option)) => LinkEvent::Selected(option.clone()),
            Some(Entry::Create(name)) => LinkEvent::Create(name.clone()),
            None => return,
        };
        let (LinkEvent::Selected(value) | LinkEvent::Create(value)) = &event;
        self.set_value(value.clone(), window, cx);
        cx.emit(event);
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
            let rows = self.entries.iter().enumerate().map(|(i, entry)| {
                let label: SharedString = match entry {
                    Entry::Option(option) => option.clone(),
                    Entry::Create(name) => format!("+ Create \"{name}\"").into(),
                };
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
                        .w(px(260.))
                        .max_h(px(240.))
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
            .child(Input::new(&self.input).small())
            .children(dropdown)
    }
}
