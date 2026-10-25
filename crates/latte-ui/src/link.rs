//! A Link or Select field: a text input with a dropdown of matching options, typed into
//! directly.
//!
//! Behaviour follows Books' `src/components/Controls/AutoComplete.vue`, `Link.vue`,
//! `Select.vue` and `src/components/Dropdown.vue` @ v0.36.0: the list opens on focus, on
//! click and while typing, and shows every option until something is typed; then the
//! options that fuzzy-match it (`fuzzyMatch`), closest first. ↑/↓ move the highlight, which
//! starts on nothing; Enter picks it, or the only option; Tab, Esc and a click elsewhere
//! close the list. Leaving the input picks the option named by what was typed, else the
//! closest match. Options with a group (`groupBy`) are listed under its heading.
//!
//! A Link field (made [`LinkInput::link`]) also ends its list with Books' entries:
//! "Create", with what was typed in a blue badge, which asks the form for a new document
//! ([`LinkCreate`]); "No results found"; and "No results found, disable filters" when
//! SystemSettings allows it ([`LinkDisableFilters`]). With a value it shows Books' chevron,
//! which opens the linked document and, hovered for 300ms, shows its quick view
//! (`QuickView.vue`).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::widgets::{badge, control_frame, popover_panel, selector_icon};

/// An option was picked: its value.
pub struct LinkPicked(pub SharedString);

/// The chevron of a Link field was clicked: open the linked document, by its name.
pub struct LinkOpened(pub SharedString);

/// "Create" was picked: a new document, named by what was typed (empty for none).
pub struct LinkCreate(pub SharedString);

/// "No results found, disable filters" was picked: the field's filters are to go.
pub struct LinkDisableFilters;

/// An option: its value and what is shown for it.
#[derive(Clone)]
pub struct LinkOption {
    pub value: SharedString,
    pub label: SharedString,
}

/// What `QuickView.vue` shows of a linked document: its name (unless named at random),
/// its schema's label and its values.
pub struct QuickViewData {
    pub name: Option<String>,
    pub schema_label: String,
    pub values: Vec<(String, String)>,
}

/// Reads a linked document's quick view, by its name.
pub type QuickViewFn = Rc<dyn Fn(&str, &App) -> QuickViewData>;

/// A row of the dropdown (`Dropdown.vue`'s `dropdownItems`).
#[derive(Clone)]
enum Item {
    Option(usize),
    Group(SharedString),
    Create,
    NoResults,
    DisableFilters,
}

pub struct LinkInput {
    input: Entity<InputState>,
    options: Vec<LinkOption>,
    /// Each option's group, by its value (`groupBy`).
    groups: HashMap<SharedString, SharedString>,
    items: Vec<Item>,
    highlighted: Option<usize>,
    open: bool,
    value: Option<SharedString>,
    /// What was typed since the input was focused; until then every option shows.
    keyword: Option<String>,
    /// Drawn in Books' bordered frame (a form field), or bare (a table cell).
    bordered: bool,
    /// A Link field, whose value can be opened.
    linkable: bool,
    /// A Link field (`Link.vue`): "No results found" and "Create" are offered.
    is_link: bool,
    /// `df.create`: "Create" ends the list.
    create: bool,
    /// The options are filtered and SystemSettings' `allowFilterBypass` is on.
    bypass: bool,
    quick_view: Option<QuickViewFn>,
    /// The chevron is hovered: the quick view shows after 300ms.
    hovering: bool,
    show_quick_view: bool,
    /// Required and empty: the selector is red (`showMandatory`).
    mandatory: bool,
    /// `size="small"`: `px-2 py-1`, as report filters are.
    small: bool,
    scroll: ScrollHandle,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<LinkPicked> for LinkInput {}
impl EventEmitter<LinkOpened> for LinkInput {}
impl EventEmitter<LinkCreate> for LinkInput {}
impl EventEmitter<LinkDisableFilters> for LinkInput {}

impl LinkInput {
    pub fn new(
        options: Vec<LinkOption>,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        // Tab leaves the field in Books, which settles what was typed. The input and the
        // window's root take Tab as an action, so it is caught before them.
        let this = cx.entity().downgrade();
        let tab = cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key != "tab" {
                return;
            }
            this.update(cx, |this, cx| {
                if this.is_focused(window, cx)
                    && let Some(value) = this.commit(window, cx)
                {
                    cx.emit(LinkPicked(value));
                    cx.notify();
                }
            })
            .ok();
        });
        Self {
            input,
            options,
            groups: HashMap::new(),
            items: Vec::new(),
            highlighted: None,
            open: false,
            value: None,
            keyword: None,
            bordered: true,
            linkable: false,
            is_link: false,
            create: false,
            bypass: false,
            quick_view: None,
            hovering: false,
            show_quick_view: false,
            mandatory: false,
            small: false,
            scroll: ScrollHandle::new(),
            _subscriptions: [subscription, tab],
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

    /// A Link field (`Link.vue`), which says when nothing matches; with `create`, its
    /// list ends with "Create".
    pub fn link(mut self, create: bool) -> Self {
        self.is_link = true;
        self.create = create;
        self
    }

    /// Reads the quick view the chevron shows.
    pub fn quick_view(mut self, quick_view: QuickViewFn) -> Self {
        self.quick_view = Some(quick_view);
        self
    }

    pub fn set_mandatory(&mut self, mandatory: bool) {
        self.mandatory = mandatory;
    }

    /// Whether "No results found, disable filters" is offered when nothing matches.
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    /// Each option's group, by its value.
    pub fn set_groups(&mut self, groups: HashMap<SharedString, SharedString>) {
        self.groups = groups;
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.value.as_ref()
    }

    pub fn set_options(&mut self, options: Vec<LinkOption>, cx: &mut Context<Self>) {
        self.options = options;
        self.refilter(cx);
    }

    /// Opens the list again with every option, as after "disable filters".
    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
        // `disableFiltering` asks for the suggestions with no keyword: all of them.
        self.keyword = None;
        self.refilter(cx);
        self.open = true;
        cx.notify();
    }

    fn label_of(&self, value: &str) -> SharedString {
        self.options
            .iter()
            .find(|option| option.value == value)
            .map_or_else(|| SharedString::from(value.to_owned()), |o| o.label.clone())
    }

    /// Sets the value without emitting [`LinkPicked`], as a field set by another field
    /// would.
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

    /// Something was typed since the input was focused, which a value set by another
    /// field mustn't overwrite.
    pub fn is_typing(&self) -> bool {
        self.keyword.is_some()
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }

    fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_owned()
    }

    /// The options matching `keyword`, closest first (`getSuggestions`). A Select field
    /// matches it lowercased, as `AutoComplete.vue` does.
    fn matching(&self, keyword: &str) -> Vec<usize> {
        let keyword = if self.is_link {
            keyword.to_owned()
        } else {
            keyword.to_lowercase()
        };
        let labels: Vec<&str> = self.options.iter().map(|o| o.label.as_ref()).collect();
        latte_core::views::form::suggestions(&labels, &keyword)
    }

    /// `getSuggestions` and `dropdownItems`: the options, grouped, then Books' entries.
    fn refilter(&mut self, cx: &App) {
        let keyword = self.keyword.clone().unwrap_or_default();
        let matched = self.matching(&keyword);
        let mut items: Vec<Item> = Vec::new();
        let mut grouped: Vec<(SharedString, Vec<usize>)> = Vec::new();
        for i in &matched {
            match self.groups.get(&self.options[*i].value) {
                Some(group) => match grouped.iter_mut().find(|(g, _)| g == group) {
                    Some((_, list)) => list.push(*i),
                    None => grouped.push((group.clone(), vec![*i])),
                },
                None => items.push(Item::Option(*i)),
            }
        }
        grouped.sort_by(|a, b| a.0.cmp(&b.0));
        for (group, list) in grouped {
            items.push(Item::Group(group));
            items.extend(list.into_iter().map(Item::Option));
        }
        if self.is_link && matched.is_empty() {
            if self.bypass {
                items.push(Item::DisableFilters);
            } else if !self.create {
                items.push(Item::NoResults);
            }
        }
        if self.is_link && self.create {
            items.push(Item::Create);
        }
        let _ = cx;
        self.items = items;
        // `dropdownItems` watch: the highlight stays, within the list.
        self.highlighted = self
            .highlighted
            .filter(|_| !self.items.is_empty())
            .map(|h| h.min(self.items.len() - 1));
    }

    fn open_list(&mut self, cx: &App) {
        self.refilter(cx);
        self.open = true;
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
                self.keyword = None;
                self.open_list(cx);
            }
            InputEvent::Change => {
                let typed = self.input.read(cx).value().to_string();
                // `onInput`: emptied, the list closes.
                if typed.is_empty() {
                    self.keyword = Some(String::new());
                    self.open = false;
                } else {
                    self.keyword = Some(typed);
                    self.open_list(cx);
                }
            }
            InputEvent::Blur => self.on_blur(window, cx),
            InputEvent::PressEnter { .. } => {}
        }
        cx.notify();
    }

    /// `onBlur`: what was typed picks the option it names, else the closest; with
    /// nothing typed the value is cleared.
    fn on_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(value) = self.commit(window, cx) {
            cx.emit(LinkPicked(value));
        }
    }

    /// Settles what was typed as `onBlur` does, and closes the list; returns the value
    /// picked (empty when cleared) if it changed. The form calls it before it saves, as
    /// Books' input is blurred first.
    pub fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<SharedString> {
        self.open = false;
        self.highlighted = None;
        let typed = self.query(cx);
        let keyword = self.keyword.take();
        if typed.is_empty() {
            if self.value.is_some() {
                self.value = None;
                return Some(SharedString::default());
            }
            return None;
        }
        // Nothing was typed: the value stays.
        keyword.as_ref()?;
        let found = self
            .options
            .iter()
            .position(|o| o.label == typed)
            .or_else(|| self.matching(&typed).first().copied());
        match found {
            Some(i) => {
                let value = self.options[i].value.clone();
                let changed = self.value.as_ref() != Some(&value);
                self.value = None;
                self.set_value(Some(value.clone()), window, cx);
                changed.then_some(value)
            }
            // A Select with nothing that matches takes what was typed, which its
            // document then refuses, as Books' `triggerChange(label)`.
            None if !self.is_link => Some(typed.into()),
            None => {
                self.restore(window, cx);
                None
            }
        }
    }
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self
            .value
            .as_deref()
            .map(|v| self.label_of(v))
            .unwrap_or_default();
        self.input
            .update(cx, |input, cx| input.set_value(label, window, cx));
    }

    /// `selectItem`.
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(index).cloned() else {
            return;
        };
        match item {
            Item::Option(i) => {
                let value = self.options[i].value.clone();
                self.value = None;
                self.keyword = None;
                self.set_value(Some(value.clone()), window, cx);
                cx.emit(LinkPicked(value));
            }
            Item::Group(_) => return,
            Item::Create => {
                let typed: SharedString = self.keyword.clone().unwrap_or_default().into();
                self.open = false;
                self.keyword = None;
                self.restore(window, cx);
                cx.emit(LinkCreate(typed));
            }
            Item::NoResults => {
                self.open = false;
            }
            Item::DisableFilters => {
                self.open = false;
                cx.emit(LinkDisableFilters);
            }
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.items.len();
        match event.keystroke.key.as_str() {
            "down" => {
                if !self.open {
                    self.open_list(cx);
                }
                if !self.items.is_empty() {
                    let next = self.highlighted.map_or(0, |h| h + 1);
                    self.highlighted = Some(next.min(self.items.len() - 1));
                }
            }
            "up" => {
                if !self.open {
                    self.open_list(cx);
                }
                if !self.items.is_empty() {
                    self.highlighted = Some(self.highlighted.unwrap_or(0).saturating_sub(1));
                }
            }
            "enter" if self.open && count > 0 => {
                // `selectHighlightedItem`: the highlighted item, or the only one.
                match self.highlighted {
                    Some(h) => self.choose(h, window, cx),
                    None if count == 1 => self.choose(0, window, cx),
                    None => {}
                }
                self.open = false;
            }
            "escape" if self.open => self.open = false,
            _ => return,
        }
        if let Some(h) = self.highlighted {
            self.scroll.scroll_to_item(h);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn hover_chevron(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hovering = hovered;
        if !hovered {
            self.show_quick_view = false;
            cx.notify();
            return;
        }
        // `Popover`'s `entry-delay`.
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            this.update(cx, |this, cx| {
                if this.hovering {
                    this.show_quick_view = true;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// `Dropdown.vue`'s list, under the input (its parent is `relative`).
    fn render_dropdown(&self, cx: &mut Context<Self>) -> Div {
        let rows: Vec<AnyElement> = if self.items.is_empty() {
            // `getEmptyMessage`.
            vec![
                div()
                    .p_2()
                    .italic()
                    .text_color(gray(600))
                    .child(t("Empty"))
                    .into_any_element(),
            ]
        } else {
            self.items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    if let Item::Group(group) = item {
                        return div()
                            .px_2()
                            .pt_3()
                            .pb_1()
                            .text_11()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(700))
                            .child(group.to_uppercase())
                            .into_any_element();
                    }
                    let content: AnyElement = match item {
                        Item::Option(o) => self.options[*o].label.clone().into_any_element(),
                        Item::Create => {
                            let typed = self.keyword.clone().unwrap_or_default();
                            let is_new = !typed.is_empty()
                                && !self.items.iter().any(|i| {
                                    matches!(i, Item::Option(o) if self.options[*o].label == typed)
                                });
                            div()
                                .h_flex()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t("Create"))
                                .when(is_new, |d| {
                                    d.child(
                                        badge(typed, "blue", cx)
                                            .ml_2()
                                            .font_weight(FontWeight::NORMAL),
                                    )
                                })
                                .into_any_element()
                        }
                        Item::NoResults => div()
                            .text_color(gray(600))
                            .child(t("No results found"))
                            .into_any_element(),
                        Item::DisableFilters => div()
                            .text_color(gray(600))
                            .child(t("No results found, disable filters"))
                            .into_any_element(),
                        Item::Group(_) => unreachable!(),
                    };
                    div()
                        .id(i)
                        .p_2()
                        .when(i > 0, |d| d.mt_1())
                        .rounded(books::RADIUS_MD)
                        .truncate()
                        .cursor_pointer()
                        .when(self.highlighted == Some(i), |row| row.bg(gray(100)))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.highlighted != Some(i) {
                                this.highlighted = Some(i);
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
                        .child(content)
                        .into_any_element()
                })
                .collect()
        };
        // `anchored` is laid out absolutely, so it goes in a corner of its own.
        div().absolute().left_0().bottom_0().size_0().child(
            deferred(
                anchored().snap_to_window().child(
                    popover_panel()
                        .id("link-dropdown")
                        .occlude()
                        .mt_2()
                        .min_w(px(160.))
                        .max_h(px(256.))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .p_1()
                        .text_12()
                        .children(rows),
                ),
            )
            .with_priority(1),
        )
    }
}

/// `QuickView.vue`: the name and schema, then a pill for each value.
fn render_quick_view(data: QuickViewData) -> impl IntoElement {
    let has_values = !data.values.is_empty();
    popover_panel()
        .min_w(px(192.))
        .max_w(px(300.))
        .text_13()
        .child(
            div()
                .p_2()
                .h_flex()
                .justify_between()
                .gap_2()
                .when(has_values, |d| d.border_b_1().border_color(gray(200)))
                .children(data.name.map(|name| {
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gray(900))
                        .child(name)
                }))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gray(600))
                        .child(data.schema_label),
                ),
        )
        .when(has_values, |d| {
            d.child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_2()
                    .p_2()
                    .children(data.values.into_iter().map(|(label, value)| {
                        div()
                            .h_flex()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(books::RADIUS_MD)
                            .bg(gray(200))
                            .text_11()
                            .child(div().text_color(gray(600)).child(label))
                            .child(div().ml(px(6.)).text_color(gray(800)).child(value))
                    })),
            )
        })
}

impl Render for LinkInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.is_focused(window, cx);
        let dropdown = self.open.then(|| self.render_dropdown(cx));

        let quick_view = self
            .show_quick_view
            .then(|| self.value.clone())
            .flatten()
            .and_then(|value| self.quick_view.as_ref().map(|f| f(&value, cx)));
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
                    .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                        this.hover_chevron(*hovered, window, cx)
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_quick_view = false;
                        this.hovering = false;
                        cx.emit(LinkOpened(value.clone()))
                    }))
                    .children(quick_view.map(|data| {
                        deferred(
                            anchored()
                                .snap_to_window()
                                .anchor(Anchor::TopRight)
                                .child(div().mt_6().child(render_quick_view(data))),
                        )
                        .with_priority(1)
                    }))
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
            .relative()
            .w_full()
            .capture_key_down(cx.listener(Self::on_key_down))
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if !this.open {
                    this.open_list(cx);
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
