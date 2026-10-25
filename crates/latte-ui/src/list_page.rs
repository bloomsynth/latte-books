//! A list of documents: `src/pages/ListView/ListView.vue` and `List.vue` @ v0.36.0.
//!
//! The header holds Books' Filter menu (`FilterDropdown.vue`) and New. A list opened for
//! some documents only, such as a party's invoices, starts with those filters.

use crate::filter_dropdown::{FilterDropdown, FiltersChanged};
use crate::widgets::Button;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_core::views::list::{self, ListCell, ListColumn, ListRow};
use latte_core::views::sidebar::Route;
use latte_db::{Filter, FilterValue, QueryFilter};
use latte_schema::FieldType;

use crate::books::{self, Text as _, gray, white};
use crate::i18n::t;
use crate::session::Shared;
use crate::widgets::{badge, error_message, page_header};
use crate::{Nav, Navigate};

pub struct ListPage {
    session: Shared,
    schema: String,
    title: String,
    filters_name: Option<String>,
    filters: QueryFilter,
    /// The Filter menu's filters, on top of `filters`.
    conditions: Vec<(String, String, String)>,
    filter: Entity<FilterDropdown>,
    columns: Vec<ListColumn>,
    rows: Vec<ListRow>,
    /// `Paginator.vue`: the page shown, from 1, and the rows on a page.
    page: usize,
    count: usize,
    error: Option<String>,
    _subscription: Subscription,
}

impl EventEmitter<Navigate> for ListPage {}

impl ListPage {
    pub fn new(
        session: Shared,
        schema: &str,
        title: &str,
        filters_name: Option<&str>,
        conditions: Vec<(String, String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields = session
            .borrow()
            .fyo
            .schema(schema)
            .map(|schema| schema.fields.clone())
            .unwrap_or_default();
        let filter = cx.new(|cx| FilterDropdown::new(&fields, &conditions, window, cx));
        let subscription = cx.subscribe(&filter, |this, _, event: &FiltersChanged, cx| {
            this.conditions = event.0.clone();
            this.page = 1;
            this.load(cx);
        });
        let columns = list::columns(&session.borrow().fyo, schema);
        let filters = filters_name.map(list::route_filters).unwrap_or_default();
        let mut page = Self {
            session,
            schema: schema.to_owned(),
            title: title.to_owned(),
            filters_name: filters_name.map(str::to_owned),
            filters,
            conditions,
            filter,
            columns,
            rows: Vec::new(),
            page: 1,
            count: COUNTS[0] as usize,
            error: None,
            _subscription: subscription,
        };
        page.load(cx);
        page
    }

    /// The route's filters and the Filter menu's, as a query filter.
    fn query_filter(&self) -> QueryFilter {
        let s = self.session.borrow();
        let schema = s.fyo.schema(&self.schema);
        let mut filters = self.filters.clone();
        for (field, operator, value) in &self.conditions {
            let fieldtype = schema
                .and_then(|schema| schema.field(field))
                .and_then(|f| f.fieldtype);
            // Money is stored as text with every digit of its precision, which a
            // filter on it is given in too (`fyo.pesa(0).store`).
            let value = match (fieldtype, value.parse::<f64>()) {
                (Some(FieldType::Currency), Ok(n)) => match s.fyo.pesa().from_f64(n) {
                    Ok(money) => FilterValue::Text(money.store()),
                    Err(_) => FilterValue::Text(value.clone()),
                },
                (Some(FieldType::Int | FieldType::Float), Ok(n)) => FilterValue::Number(n),
                _ => FilterValue::Text(value.clone()),
            };
            let filter = match operator.as_str() {
                "is null" => Filter::Equals(FilterValue::Null),
                "is not null" => Filter::op("!=", FilterValue::Null),
                operator => Filter::op(operator, value),
            };
            // A number series filters the names (`emitFilterChange`).
            let field = if field == "numberSeries" {
                "name"
            } else {
                field
            };
            filters.insert(field.to_owned(), filter);
        }
        filters
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let filters = self.query_filter();
        let s = self.session.borrow();
        match list::rows(&s.fyo, &s.formatter, &self.schema, &filters, "") {
            Ok(rows) => {
                self.rows = rows;
                self.page = self.page.clamp(1, self.page_count().max(1));
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn new_doc(&mut self, cx: &mut Context<Self>) {
        let values = self
            .filters_name
            .as_deref()
            .map(list::create_values)
            .unwrap_or_default();
        let doc = self.session.borrow().fyo.get_new_doc(&self.schema, values);
        match doc {
            Ok(doc) => cx.emit(Navigate(Nav::Doc(doc))),
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
}

/// A column of `Row.vue`'s grid: equal widths, 16px apart; numbers to the right.
fn column_cell(column: &ListColumn, last: bool) -> Div {
    div()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .h_flex()
        .overflow_hidden()
        .whitespace_nowrap()
        .when(column.is_numeric(), |d| d.justify_end())
        .when(last, |d| d.pr_4())
}

/// `List.vue`'s `#` column: 32px, right-aligned, 8px before the columns.
fn number_cell() -> Div {
    div()
        .w_8()
        .mr_2()
        .flex_none()
        .h_flex()
        .justify_end()
        .text_color(gray(700))
}

/// `Paginator.vue`'s page sizes; -1 is All.
const COUNTS: [i64; 4] = [50, 100, 500, -1];

impl ListPage {
    fn page_count(&self) -> usize {
        self.rows.len().div_ceil(self.count.max(1))
    }

    fn set_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page = page.clamp(1, self.page_count().max(1));
        cx.notify();
    }

    /// `Paginator.vue`: the rows shown, the page, and the page sizes.
    fn render_paginator(&self, cx: &mut Context<Self>) -> Div {
        let total = self.rows.len();
        let first = (self.page - 1) * self.count + 1;
        let last = (self.page * self.count).min(total);
        let pages = self.page_count();
        let arrow = |id: &'static str, icon: &'static str, enabled: bool| {
            div()
                .id(id)
                .when(enabled, |d| d.cursor_pointer())
                .child(books::icon(
                    icon,
                    px(16.),
                    if enabled {
                        gray(600)
                    } else {
                        white().opacity(0.)
                    },
                ))
        };
        let counts: Vec<i64> = COUNTS
            .into_iter()
            .filter(|&c| {
                if c == -1 {
                    total >= COUNTS[0] as usize
                } else {
                    total >= c as usize
                }
            })
            .collect();
        let page = self.page;
        div()
            .h_flex()
            .h(px(50.))
            .px_4()
            .border_t_1()
            .border_color(gray(200))
            .text_12()
            .text_color(gray(800))
            .child(div().flex_1().child(format!("{first} - {last}")))
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .justify_center()
                    .gap_1()
                    .child(arrow("page-back", "chevron-left", page > 1).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.set_page(page.saturating_sub(1), cx)
                        }),
                    ))
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .bg(gray(100))
                            .rounded(books::RADIUS)
                            .child(div().w_7().h_flex().justify_end().child(page.to_string()))
                            .child(div().text_color(gray(600)).child("/"))
                            .child(div().w_7().child(pages.to_string())),
                    )
                    .child(
                        arrow("page-next", "chevron-right", page < pages).on_click(
                            cx.listener(move |this, _, _, cx| this.set_page(page + 1, cx)),
                        ),
                    ),
            )
            .child(
                div().flex_1().h_flex().justify_end().child(
                    div()
                        .h_flex()
                        .when(!counts.is_empty(), |d| {
                            d.border_1().border_color(gray(100)).rounded(books::RADIUS)
                        })
                        .children(counts.into_iter().map(|c| {
                            let size = if c == -1 { total.max(1) } else { c as usize };
                            let selected = self.count == size;
                            div()
                                .id(SharedString::from(format!("count-{c}")))
                                .w_9()
                                .h_flex()
                                .justify_center()
                                .cursor_pointer()
                                .when(selected, |d| d.bg(gray(100)).rounded(books::RADIUS))
                                .child(if c == -1 { t("All") } else { c.to_string() })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.count = size;
                                    this.page = 1;
                                    cx.notify();
                                }))
                        })),
                ),
            )
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = page_header(self.title.clone(), cx).child(
            div().h_flex().gap_2().child(self.filter.clone()).child(
                Button::new("new")
                    .primary()
                    .icon("plus")
                    .on_click(cx.listener(|this, _, _, cx| this.new_doc(cx))),
            ),
        );
        let columns = self.columns.clone();
        let n = columns.len();
        let column_header = div()
            .h_flex()
            .w_full()
            .h_12()
            .flex_none()
            .border_b_1()
            .border_color(gray(200))
            .text_color(gray(700))
            .child(number_cell().child("#"))
            .child(
                div().flex_1().h_flex().gap_4().children(
                    columns
                        .iter()
                        .enumerate()
                        .map(|(i, c)| column_cell(c, i + 1 == n).child(c.label())),
                ),
            );

        let body = if self.rows.is_empty() {
            div()
                .flex_1()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_3()
                .child(div().text_color(gray(800)).child(t("No entries found")))
                .child(
                    Button::new("make-entry")
                        .primary()
                        .label(t("Make Entry"))
                        .on_click(cx.listener(|this, _, _, cx| this.new_doc(cx))),
                )
                .into_any_element()
        } else {
            let start = (self.page - 1) * self.count;
            let end = (start + self.count).min(self.rows.len());
            let rows: Vec<ListRow> = self.rows[start..end].to_vec();
            let schema = self.schema.clone();
            uniform_list(
                "rows",
                rows.len(),
                cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|i| {
                            let row = &rows[i];
                            let route = Route::Form {
                                schema: schema.clone(),
                                name: Some(row.name.clone()),
                            };
                            div()
                                .id(i)
                                .h_flex()
                                .w_full()
                                .h_12()
                                .border_b_1()
                                .border_color(gray(200))
                                .hover(|d| d.bg(gray(50)))
                                .child(number_cell().child(format!("{}", start + i + 1)))
                                .child(
                                    div()
                                        .id(("cells", i))
                                        .flex_1()
                                        .h_full()
                                        .h_flex()
                                        .gap_4()
                                        .cursor_pointer()
                                        .text_color(gray(900))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(Navigate(Nav::Route(route.clone())))
                                        }))
                                        .children(
                                            row.cells.iter().zip(columns.iter()).enumerate().map(
                                                |(c, (cell, column))| {
                                                    let content = match cell {
                                                        ListCell::Text(text) => div()
                                                            .truncate()
                                                            .child(text.clone())
                                                            .into_any_element(),
                                                        ListCell::Status(status)
                                                            if status.label.is_empty() =>
                                                        {
                                                            div().into_any_element()
                                                        }
                                                        ListCell::Status(status) => badge(
                                                            status.label.clone(),
                                                            status.color,
                                                            cx,
                                                        )
                                                        .into_any_element(),
                                                    };
                                                    column_cell(column, c + 1 == n).child(content)
                                                },
                                            ),
                                        ),
                                )
                        })
                        .collect()
                }),
            )
            .flex_1()
            .into_any_element()
        };

        div()
            .v_flex()
            .size_full()
            .text_13()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(column_header)
            .child(body)
            .when(!self.rows.is_empty(), |d| {
                d.child(self.render_paginator(cx))
            })
    }
}
