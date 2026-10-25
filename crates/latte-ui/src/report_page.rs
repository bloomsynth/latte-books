//! A report: `src/pages/Report.vue` and `src/components/Report/ListReport.vue` @ v0.36.0.
//!
//! The filters run the report again as they change, and Export saves it as CSV.
//!
//! Differences from Books:
//! - Dates are typed as YYYY-MM-DD.
//! - Export saves CSV, and JSON for the GSTR reports; Books also offers a PDF print.

use std::collections::HashMap;

use crate::widgets::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use jiff::civil::Date;
use latte_core::reports::{
    self, Align, CellColor, Column, Filter, FilterKind, FilterValue, Report, ReportRow,
};
use latte_db::{DocValue, GetAllOptions};

use crate::Navigate;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::{error_message, info_message, page_header};

enum FilterControl {
    Input(Entity<InputState>),
    Link(Entity<LinkInput>),
    Check,
}

pub struct ReportPage {
    session: Shared,
    report: Box<dyn Report>,
    filters: Vec<Filter>,
    controls: HashMap<String, FilterControl>,
    columns: Vec<Column>,
    rows: Vec<ReportRow>,
    error: Option<String>,
    info: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for ReportPage {}

fn filter_text(value: &FilterValue) -> String {
    match value {
        FilterValue::Null => String::new(),
        FilterValue::Text(text) => text.clone(),
        FilterValue::Date(date) => date.to_string(),
        FilterValue::Int(n) => n.to_string(),
        FilterValue::Bool(b) => b.to_string(),
    }
}

impl ReportPage {
    pub fn build(name: &str) -> Option<Box<dyn Report>> {
        reports::get_report(name)
    }

    pub fn new(
        session: Shared,
        report: Box<dyn Report>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filters = report.filters(&session.borrow().fyo);
        let mut page = Self {
            session,
            report,
            filters: filters.clone(),
            controls: HashMap::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            error: None,
            info: None,
            _subscriptions: Vec::new(),
        };
        for filter in &filters {
            page.make_control(filter, window, cx);
        }
        page.run(window, cx);
        page
    }

    /// The names a Link filter offers: the documents of its schema.
    fn names(&self, schema: &str) -> Vec<LinkOption> {
        let s = self.session.borrow();
        let options = GetAllOptions {
            order_by: Some(vec!["name".to_owned()]),
            order: Some(latte_db::Order::Asc),
            ..GetAllOptions::default()
        };
        s.fyo
            .db
            .get_all_docs(schema, &options)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|values| match values.get("name") {
                Some(DocValue::Text(name)) => Some(LinkOption {
                    value: name.clone().into(),
                    label: name.clone().into(),
                }),
                _ => None,
            })
            .collect()
    }

    fn link_options(&self, filter: &Filter) -> Vec<LinkOption> {
        match &filter.kind {
            FilterKind::Select(options) => options
                .iter()
                .map(|(label, value)| LinkOption {
                    value: value.clone().into(),
                    label: label.clone().into(),
                })
                .collect(),
            FilterKind::Link(schema) => self.names(schema),
            FilterKind::DynamicLink { references } => match self.report.get(references) {
                FilterValue::Text(schema) if !schema.is_empty() => self.names(&schema),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn make_control(&mut self, filter: &Filter, window: &mut Window, cx: &mut Context<Self>) {
        let fieldname = filter.fieldname.clone();
        let value = self.report.get(&fieldname);
        let control = match &filter.kind {
            FilterKind::Check => FilterControl::Check,
            FilterKind::Date | FilterKind::Int { .. } => {
                let text = filter_text(&value);
                let label = filter.label.clone();
                let state = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(label)
                        .default_value(text)
                });
                let kind = filter.kind.clone();
                self._subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, window, cx| {
                        if !matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            return;
                        }
                        let text = state.read(cx).value().trim().to_owned();
                        let value = if text.is_empty() {
                            Ok(FilterValue::Null)
                        } else {
                            match kind {
                                FilterKind::Date => {
                                    text.parse::<Date>().map(FilterValue::Date).map_err(|_| {
                                        format!("\"{text}\" is not a date like 2026-04-01")
                                    })
                                }
                                _ => text
                                    .parse::<i64>()
                                    .map(FilterValue::Int)
                                    .map_err(|_| format!("\"{text}\" is not a whole number")),
                            }
                        };
                        match value {
                            Ok(value) => this.set_filter(&fieldname, value, window, cx),
                            Err(e) => {
                                this.error = Some(e);
                                cx.notify();
                            }
                        }
                    },
                ));
                FilterControl::Input(state)
            }
            _ => {
                let options = self.link_options(filter);
                let label = filter.label.clone();
                let state = cx.new(|cx| LinkInput::new(options, label, window, cx));
                if let FilterValue::Text(text) = &value {
                    let text: SharedString = text.clone().into();
                    state.update(cx, |link, cx| link.set_value(Some(text), window, cx));
                }
                self._subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event: &LinkPicked, window, cx| {
                        let value = if event.0.is_empty() {
                            FilterValue::Null
                        } else {
                            FilterValue::Text(event.0.to_string())
                        };
                        this.set_filter(&fieldname, value, window, cx);
                    },
                ));
                FilterControl::Link(state)
            }
        };
        self.controls.insert(filter.fieldname.clone(), control);
    }

    fn set_filter(
        &mut self,
        fieldname: &str,
        value: FilterValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.report.get(fieldname) == value {
            return;
        }
        self.report.set(fieldname, value);
        // A filter can change others (a reference type clears its name), so all are shown
        // again.
        for filter in self.filters.clone() {
            let value = self.report.get(&filter.fieldname);
            match self.controls.get(&filter.fieldname) {
                Some(FilterControl::Input(state)) => {
                    let text = filter_text(&value);
                    state.update(cx, |input, cx| {
                        if input.value() != text && !input.focus_handle(cx).is_focused(window) {
                            input.set_value(text, window, cx)
                        }
                    });
                }
                Some(FilterControl::Link(state)) => {
                    let options = self.link_options(&filter);
                    let value = match value {
                        FilterValue::Text(text) if !text.is_empty() => Some(text.into()),
                        _ => None,
                    };
                    state.update(cx, |link, cx| {
                        link.set_options(options, cx);
                        link.set_value(value, window, cx);
                    });
                }
                _ => {}
            }
        }
        self.run(window, cx);
    }

    fn run(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let s = self.session.borrow();
        let result = self
            .report
            .columns(&s.fyo, &s.formatter)
            .and_then(|columns| Ok((columns, self.report.run(&s.fyo, &s.formatter)?)));
        match result {
            Ok((columns, rows)) => {
                self.columns = columns;
                self.rows = rows;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn export(&mut self, json: bool, window: &mut Window, cx: &mut Context<Self>) {
        let data = {
            let s = self.session.borrow();
            self.report.can_export(&s.fyo).and_then(|()| {
                if json {
                    self.report
                        .export_json(&s.fyo)
                        .unwrap_or_else(|| Ok(String::new()))
                } else {
                    let precision = s.formatter.display_precision;
                    Ok(reports::csv(&self.columns, &self.rows, precision))
                }
            })
        };
        let csv = match data {
            Ok(data) if !data.is_empty() => data,
            Ok(_) => return,
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let extension = if json { "json" } else { "csv" };
        let file_name = format!("{}.{extension}", self.report.report_name());
        let dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| std::path::PathBuf::from(home).join("Documents"))
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&dir, Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, csv);
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.info = Some(format!("Saved {}", path.display())),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_filter(&self, filter: &Filter, cx: &mut Context<Self>) -> AnyElement {
        let control = match self.controls.get(&filter.fieldname) {
            Some(FilterControl::Input(state)) => Input::new(state).small().into_any_element(),
            Some(FilterControl::Link(state)) => state.clone().into_any_element(),
            Some(FilterControl::Check) | None => {
                let checked = self.report.get(&filter.fieldname) == FilterValue::Bool(true);
                let fieldname = filter.fieldname.clone();
                Checkbox::new(SharedString::from(format!("filter-{fieldname}")))
                    .checked(checked)
                    .label(filter.label.clone())
                    .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                        this.set_filter(&fieldname, FilterValue::Bool(*checked), window, cx)
                    }))
                    .into_any_element()
            }
        };
        div().w(px(170.)).child(control).into_any_element()
    }
}

fn cell_box(column: &Column) -> Div {
    let d = div()
        .flex_grow(column.width.max(0.5) as f32)
        .flex_basis(px(0.))
        .min_w(px(60.))
        .px_2()
        .truncate();
    match column.align {
        Align::Right => d.h_flex().justify_end(),
        Align::Center => d.h_flex().justify_center(),
        Align::Left => d,
    }
}

impl Render for ReportPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = page_header(t(self.report.title()), cx).child(
            div()
                .h_flex()
                .gap_2()
                .when(self.report.has_json_export(), |buttons| {
                    buttons.child(
                        Button::new("export-json")
                            .small()
                            .label(t("JSON"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.export(true, window, cx)),
                            ),
                    )
                })
                .child(
                    Button::new("export").small().label(t("Export")).on_click(
                        cx.listener(|this, _, window, cx| this.export(false, window, cx)),
                    ),
                ),
        );
        let filters: Vec<AnyElement> = self
            .filters
            .clone()
            .iter()
            .map(|f| self.render_filter(f, cx))
            .collect();
        let columns = self.columns.clone();
        let column_header = div()
            .h_flex()
            .w_full()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .text_color(theme.muted_foreground)
            .children(columns.iter().map(|c| cell_box(c).child(c.label.clone())));
        let rows = self.rows.clone();
        let body = uniform_list(
            "report-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                let theme = cx.theme().clone();
                range
                    .map(|i| {
                        let row = &rows[i];
                        div()
                            .id(i)
                            .h_flex()
                            .w_full()
                            .h(px(32.))
                            .px_4()
                            .border_b_1()
                            .border_color(theme.border)
                            .when(row.is_group, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .children(row.cells.iter().zip(columns.iter()).map(|(cell, column)| {
                                let mut d = cell_box(column).pl(px(8. + 16. * cell.indent as f32));
                                if cell.bold {
                                    d = d.font_weight(FontWeight::SEMIBOLD);
                                }
                                if cell.italics {
                                    d = d.italic();
                                }
                                match cell.color {
                                    Some(CellColor::Red) => d = d.text_color(theme.red),
                                    Some(CellColor::Green) => d = d.text_color(theme.green),
                                    None => {}
                                }
                                d.child(cell.value.clone())
                            }))
                    })
                    .collect()
            }),
        )
        .flex_1();

        div()
            .v_flex()
            .size_full()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|m| info_message(m, cx)))
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .children(filters),
            )
            .child(column_header)
            .child(body)
    }
}
