//! The point of sale's Link fields: Books' `MultiLabelLink.vue` (the item search and the
//! customer) and `Link.vue` (Filter by Group and the modals' links) @ v0.36.0, built on
//! `AutoComplete.vue`.
//!
//! The list opens on focus, on click and while typing, and says "No results found" when
//! nothing matches; ↑/↓ move the highlight, Enter picks it, Tab, Esc and a click
//! elsewhere close it. With a value the field shows Books' clear button (when asked for)
//! and the chevron to the linked document. Enter is also passed on with the text typed,
//! as the POS's `@keyup.enter` reads it, after the highlighted option is picked.
//!
//! As `link.rs` does, matching is a case-insensitive substring match, not fuzzy.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::widgets::{control_frame, popover_panel, selector_icon};

/// What happened in a [`PosLink`].
pub enum PosLinkEvent {
    /// An option was picked, or the value cleared (empty).
    Picked(SharedString),
    /// Enter was pressed: the text in the field then.
    Entered(SharedString),
    /// The chevron was clicked: open the linked document.
    Opened(SharedString),
}

pub struct PosLink {
    input: Entity<InputState>,
    options: Vec<SharedString>,
    entries: Vec<usize>,
    highlighted: usize,
    open: bool,
    value: Option<SharedString>,
    clearable: bool,
    linkable: bool,
    _subscription: Subscription,
}

impl EventEmitter<PosLinkEvent> for PosLink {}

impl PosLink {
    pub fn new(
        options: Vec<String>,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        Self {
            input,
            options: options.into_iter().map(Into::into).collect(),
            entries: Vec::new(),
            highlighted: 0,
            open: false,
            value: None,
            clearable: false,
            linkable: true,
            _subscription: subscription,
        }
    }

    /// `show-clear-button`.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.value.as_ref()
    }

    pub fn text(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.focus_handle(cx).focus(window, cx);
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }

    /// Sets the value without emitting [`PosLinkEvent::Picked`].
    pub fn set_value(
        &mut self,
        value: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = value.clone().unwrap_or_default();
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.value = value.filter(|v| !v.is_empty());
        self.open = false;
        cx.notify();
    }

    fn refilter(&mut self, cx: &App) {
        let query = self.input.read(cx).value();
        let needle = query.trim().to_lowercase();
        let showing_value = self
            .value
            .as_deref()
            .is_some_and(|v| v.to_lowercase() == needle);
        self.entries = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, option)| showing_value || option.to_lowercase().contains(&needle))
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
                let typed = self.input.read(cx).value().trim().to_owned();
                let found = self
                    .options
                    .iter()
                    .find(|o| o.eq_ignore_ascii_case(&typed))
                    .cloned();
                match found {
                    Some(value) if self.value.as_ref() != Some(&value) => {
                        self.set_value(Some(value.clone()), window, cx);
                        cx.emit(PosLinkEvent::Picked(value));
                    }
                    Some(_) => {}
                    None if typed.is_empty() && self.value.is_some() => {
                        self.value = None;
                        cx.emit(PosLinkEvent::Picked(SharedString::default()));
                    }
                    None => {}
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
        let value = option.clone();
        self.set_value(Some(value.clone()), window, cx);
        cx.emit(PosLinkEvent::Picked(value));
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_value(None, window, cx);
        cx.emit(PosLinkEvent::Picked(SharedString::default()));
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
            "enter" => {
                if self.open && count > 0 {
                    self.choose(self.highlighted, window, cx);
                }
                self.open = false;
                let text = self.input.read(cx).value();
                cx.emit(PosLinkEvent::Entered(text));
            }
            "escape" | "tab" if self.open => {
                self.open = false;
                cx.notify();
                return;
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for PosLink {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.is_focused(window, cx);
        // `Dropdown.vue`: items `p-2 rounded-md`, the highlighted one gray-100.
        let dropdown = self.open.then(|| {
            let rows: Vec<AnyElement> = if self.entries.is_empty() {
                vec![
                    div()
                        .p_2()
                        .text_color(gray(600))
                        .child(t("No results found"))
                        .into_any_element(),
                ]
            } else {
                self.entries
                    .iter()
                    .enumerate()
                    .map(|(i, option)| {
                        let label = self.options[*option].clone();
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
                            .into_any_element()
                    })
                    .collect()
            };
            deferred(
                anchored().snap_to_window().child(
                    popover_panel()
                        .id("pos-link-dropdown")
                        .occlude()
                        .mt_1()
                        .min_w(px(160.))
                        .max_h(px(256.))
                        .overflow_y_scroll()
                        .p_1()
                        .text_12()
                        .children(rows),
                ),
            )
            .with_priority(2)
        });

        let value = self.value.clone();
        let buttons = (self.linkable).then_some(value).flatten().map(|value| {
            div()
                .h_flex()
                .gap_1()
                .when(self.clearable, |d| {
                    d.child(
                        div()
                            .id("clear")
                            .p(px(2.))
                            .rounded(books::RADIUS)
                            .cursor_pointer()
                            .child(books::icon("x", px(14.), gray(600)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                    )
                })
                .child(
                    div()
                        .id("open-link")
                        .p(px(2.))
                        .rounded(books::RADIUS)
                        .cursor_pointer()
                        .child(books::icon("chevron-right", px(16.), gray(600)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(PosLinkEvent::Opened(value.clone()))
                        })),
                )
        });
        let has_buttons = buttons.is_some();

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
                control_frame(false, focused)
                    .pr_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.input)
                                .appearance(false)
                                .h(px(32.))
                                .px_3()
                                .text_13(),
                        ),
                    )
                    .children(buttons)
                    .when(!has_buttons, |d| d.child(selector_icon(false))),
            )
            // `anchored` is placed absolutely: under the field, from a line after it.
            .child(div().relative().w_full().h(px(0.)).children(dropdown))
    }
}
