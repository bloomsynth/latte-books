//! A list's Filter button and its menu: `src/components/FilterDropdown.vue` @ v0.36.0.
//!
//! Each filter is a field, a condition and a value. The filters with a value apply when
//! Apply is clicked or the menu closes; the button then says how many apply.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_schema::{Field, FieldType};

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::link::{LinkInput, LinkOption};
use crate::widgets::{popover_panel, small_input};

/// `conditions`: the label shown and the operator it filters with.
const CONDITIONS: [(&str, &str); 8] = [
    ("Is", "="),
    ("Is Not", "!="),
    ("Contains", "like"),
    ("Does Not Contain", "not like"),
    ("Greater Than", ">"),
    ("Less Than", "<"),
    ("Is Empty", "is null"),
    ("Is Not Empty", "is not null"),
];

/// The filters to apply: (field, operator, value).
pub struct FiltersChanged(pub Vec<(String, String, String)>);

struct Row {
    field: Entity<LinkInput>,
    condition: Entity<LinkInput>,
    value: Entity<InputState>,
}

pub struct FilterDropdown {
    /// `fields`: the fields a list can be filtered by.
    fields: Vec<String>,
    rows: Vec<Row>,
    /// How many filters apply now.
    applied: usize,
    open: bool,
}

impl EventEmitter<FiltersChanged> for FilterDropdown {}

impl FilterDropdown {
    /// The menu for a schema's `fields`, starting with `initial` (such as a party's
    /// invoices).
    pub fn new(
        fields: &[Field],
        initial: &[(String, String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields = fields
            .iter()
            .filter(|f| {
                f.filter
                    || !(matches!(
                        f.fieldtype,
                        Some(FieldType::Table | FieldType::Attachment | FieldType::AttachImage)
                    ) || f.computed
                        || f.meta
                        || f.read_only)
            })
            .map(|f| f.fieldname.clone())
            .collect();
        let mut dropdown = Self {
            fields,
            rows: Vec::new(),
            applied: 0,
            open: false,
        };
        for (field, operator, value) in initial {
            dropdown.add_row(field, operator, value, window, cx);
        }
        dropdown.applied = dropdown.filters(cx).len();
        dropdown
    }

    fn add_row(
        &mut self,
        field: &str,
        operator: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Books offers the fields by their names.
        let field_options = self
            .fields
            .iter()
            .map(|f| LinkOption {
                value: f.clone().into(),
                label: f.clone().into(),
            })
            .collect();
        let condition_options = CONDITIONS
            .iter()
            .map(|(label, operator)| LinkOption {
                value: (*operator).into(),
                label: t(label).into(),
            })
            .collect();
        let field_input =
            cx.new(|cx| LinkInput::new(field_options, t("Field"), window, cx).small());
        let condition =
            cx.new(|cx| LinkInput::new(condition_options, t("Condition"), window, cx).small());
        let value_input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Value")));
        let field = SharedString::from(field.to_owned());
        let operator = SharedString::from(operator.to_owned());
        let value = value.to_owned();
        field_input.update(cx, |input, cx| input.set_value(Some(field), window, cx));
        condition.update(cx, |input, cx| input.set_value(Some(operator), window, cx));
        value_input.update(cx, |input, cx| input.set_value(value, window, cx));
        // Enter in a value applies the filters (`@keydown.enter="applyFilters"`).
        cx.subscribe_in(
            &value_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.apply(cx);
                }
            },
        )
        .detach();
        self.rows.push(Row {
            field: field_input,
            condition,
            value: value_input,
        });
    }

    /// `emitFilterChange`: the filters that have a value, and those that need none.
    fn filters(&self, cx: &App) -> Vec<(String, String, String)> {
        self.rows
            .iter()
            .filter_map(|row| {
                let field = row.field.read(cx).value()?.to_string();
                let operator = row.condition.read(cx).value()?.to_string();
                let value = row.value.read(cx).value().trim().to_owned();
                let needs_none = operator.starts_with("is ");
                (needs_none || !value.is_empty()).then_some((field, operator, value))
            })
            .collect()
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let filters = self.filters(cx);
        self.applied = filters.len();
        self.open = false;
        cx.emit(FiltersChanged(filters));
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.applied = 0;
        cx.emit(FiltersChanged(Vec::new()));
        cx.notify();
    }
}

impl Render for FilterDropdown {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match self.applied {
            0 => t("Filter"),
            1 => t("1 filter applied"),
            n => format!("{n} {}", t("filters applied")),
        };
        let button = div()
            .id("filter-button")
            .h_flex()
            .h_8()
            .px_3()
            .gap_1()
            .rounded(books::RADIUS_MD)
            .bg(gray(200))
            .text_12()
            .text_color(gray(700))
            .cursor_pointer()
            .child(books::icon("filter", px(12.), gray(700)))
            .child(div().ml_1().child(label))
            .on_click(cx.listener(|this, _, _, cx| {
                if this.open {
                    this.apply(cx);
                } else {
                    this.open = true;
                    cx.notify();
                }
            }));

        let menu = self.open.then(|| {
            let rows: Vec<AnyElement> = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    let group = SharedString::from(format!("filter-row-{i}"));
                    div()
                        .h_flex()
                        .gap_2()
                        .text_13()
                        .child(
                            div()
                                .id(("remove-filter", i))
                                .group(group.clone())
                                .relative()
                                .size_4()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(gray(600))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .group_hover(group.clone(), |d| d.invisible())
                                        .child((i + 1).to_string()),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .invisible()
                                        .group_hover(group, |d| d.visible())
                                        .child(books::icon("x", px(16.), gray(800))),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rows.remove(i);
                                    cx.notify();
                                })),
                        )
                        .child(div().w_24().child(row.field.clone()))
                        .child(div().w_24().child(row.condition.clone()))
                        .child(div().w_24().child(small_input(&row.value)))
                        .into_any_element()
                })
                .collect();
            let has_rows = !rows.is_empty();
            let footer_item = |id: &'static str, icon: &'static str, label: String| {
                div()
                    .id(id)
                    .h_flex()
                    .p_2()
                    .text_13()
                    .text_color(gray(600))
                    .cursor_pointer()
                    .hover(|d| d.bg(gray(100)))
                    .child(books::icon(icon, px(16.), gray(600)))
                    .child(div().ml_2().child(label))
            };
            deferred(
                anchored().anchor(Anchor::TopRight).snap_to_window().child(
                    popover_panel()
                        .id("filter-menu")
                        .occlude()
                        .mt_2()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| this.apply(cx)))
                        .child(
                            div()
                                .p_2()
                                .when(has_rows, |d| d.v_flex().gap_2().children(rows))
                                .when(!has_rows, |d| {
                                    d.text_13()
                                        .text_color(gray(600))
                                        .child(t("No filters selected"))
                                }),
                        )
                        .child(
                            div()
                                .h_flex()
                                .justify_between()
                                .border_t_1()
                                .border_color(gray(200))
                                .child(
                                    footer_item("add-filter", "plus", t("Add a filter")).on_click(
                                        cx.listener(|this, _, window, cx| {
                                            let field =
                                                this.fields.first().cloned().unwrap_or_default();
                                            this.add_row(&field, "like", "", window, cx);
                                            cx.notify();
                                        }),
                                    ),
                                )
                                .when(has_rows, |d| {
                                    d.child(
                                        div()
                                            .h_flex()
                                            .child(
                                                footer_item("clear-filters", "trash-2", t("Clear"))
                                                    .on_click(
                                                        cx.listener(|this, _, _, cx| {
                                                            this.clear(cx)
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                footer_item("apply-filters", "search", t("Apply"))
                                                    .on_click(
                                                        cx.listener(|this, _, _, cx| {
                                                            this.apply(cx)
                                                        }),
                                                    ),
                                            ),
                                    )
                                }),
                        ),
                ),
            )
            .with_priority(1)
        });

        div().relative().child(button).children(menu)
    }
}
