//! A list of documents: `src/pages/ListView/ListView.vue` and `List.vue` @ v0.36.0.
//!
//! Differences from Books:
//! - Every matching document is listed; Books shows them a page at a time.
//! - A search box finds documents by name or title, in place of Books' filter builder.

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use latte_core::views::list::{self, ListCell, ListColumn, ListRow};
use latte_core::views::sidebar::Route;
use latte_db::QueryFilter;

use crate::session::Shared;
use crate::widgets::{badge, error_message, page_header};
use crate::{Nav, Navigate};

pub struct ListPage {
    session: Shared,
    schema: String,
    title: String,
    filters_name: Option<String>,
    filters: QueryFilter,
    columns: Vec<ListColumn>,
    rows: Vec<ListRow>,
    search: Entity<InputState>,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.load(cx);
                }
            });
        let columns = list::columns(&session.borrow().fyo, schema);
        let filters = filters_name.map(list::route_filters).unwrap_or_default();
        let mut page = Self {
            session,
            schema: schema.to_owned(),
            title: title.to_owned(),
            filters_name: filters_name.map(str::to_owned),
            filters,
            columns,
            rows: Vec::new(),
            search,
            error: None,
            _subscription: subscription,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let search = self.search.read(cx).value().to_string();
        let s = self.session.borrow();
        match list::rows(&s.fyo, &s.formatter, &self.schema, &self.filters, &search) {
            Ok(rows) => {
                self.rows = rows;
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

fn column_cell(column: &ListColumn) -> Div {
    let d = div().flex_1().min_w(px(80.)).px_2().truncate();
    if column.is_numeric() {
        d.h_flex().justify_end()
    } else {
        d
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = page_header(self.title.clone(), cx).child(
            div()
                .h_flex()
                .gap_2()
                .child(div().w(px(220.)).child(Input::new(&self.search).small()))
                .child(
                    Button::new("new")
                        .small()
                        .primary()
                        .icon(IconName::Plus)
                        .on_click(cx.listener(|this, _, _, cx| this.new_doc(cx))),
                ),
        );
        let columns = self.columns.clone();
        let column_header = div()
            .h_flex()
            .w_full()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .text_color(theme.muted_foreground)
            .child(div().w(px(40.)).child("#"))
            .children(columns.iter().map(|c| column_cell(c).child(c.label())));

        let body = if self.rows.is_empty() {
            div()
                .flex_1()
                .h_flex()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child("No entries found")
                .into_any_element()
        } else {
            let rows = self.rows.clone();
            let schema = self.schema.clone();
            uniform_list(
                "rows",
                rows.len(),
                cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                    let theme = cx.theme().clone();
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
                                .h(px(36.))
                                .px_4()
                                .border_b_1()
                                .border_color(theme.border)
                                .cursor_pointer()
                                .hover(|d| d.bg(theme.list_hover))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(Navigate(Nav::Route(route.clone())))
                                }))
                                .child(
                                    div()
                                        .w(px(40.))
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{}", i + 1)),
                                )
                                .children(row.cells.iter().zip(columns.iter()).map(
                                    |(cell, column)| {
                                        let content = match cell {
                                            ListCell::Text(text) => text.clone().into_any_element(),
                                            ListCell::Status(status) if status.label.is_empty() => {
                                                div().into_any_element()
                                            }
                                            ListCell::Status(status) => {
                                                badge(status.label.clone(), status.color, cx)
                                                    .into_any_element()
                                            }
                                        };
                                        column_cell(column).child(content)
                                    },
                                ))
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
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(column_header)
            .child(body)
    }
}
