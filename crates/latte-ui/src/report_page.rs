//! A report: `src/pages/Report.vue` and `src/components/Report/ListReport.vue` @ v0.36.0.
//!
//! The filters run the report again as they change, and Export saves it as CSV.
//!
//! As in Books, the filters are a grid of labelled controls, the rows are 48px with fixed
//! column widths, a group row folds what is under it, and the ledgers come a page at a
//! time.
//!
//! Differences from Books:
//! - Dates are typed as YYYY-MM-DD.
//! - Export saves CSV, and JSON for the GSTR reports; Books also offers a PDF print.

use std::collections::{HashMap, HashSet};

use crate::books::{self, Text as _, gray};
use crate::widgets::{
    Button, Kind as ToastKind, check_box, control_frame, dropdown_item, popover_panel, toast,
};
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
use crate::widgets::{error_message, page_header};

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
    /// The Export menu is open.
    menu: bool,
    /// The group rows folded, by index.
    folded: HashSet<usize>,
    /// `Paginator.vue`, for the ledgers: the page, from 1, and the rows on a page.
    page: usize,
    count: usize,
    _subscriptions: Vec<Subscription>,
}

/// `wconst`: a column of width 1 is 8rem.
const WCONST: f32 = 128.;

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
            menu: false,
            folded: HashSet::new(),
            page: 1,
            count: 50,
            _subscriptions: Vec::new(),
        };
        // The first run sets the default filters, which the controls then show.
        page.run(window, cx);
        for filter in &filters {
            page.make_control(filter, window, cx);
        }
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
                let state = cx.new(|cx| LinkInput::new(options, label, window, cx).small());
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
        // Running sets the default filters, such as To Date, that the columns follow.
        let result = self
            .report
            .run(&s.fyo, &s.formatter)
            .and_then(|rows| Ok((self.report.columns(&s.fyo, &s.formatter)?, rows)));
        match result {
            Ok((columns, rows)) => {
                self.columns = columns;
                self.rows = rows;
                self.folded.clear();
                self.page = 1;
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
                    Ok(()) => this.info = Some(format!("Export Successful: {}", path.display())),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A filter as `Report.vue` shows it: its label above a small bordered control.
    fn render_filter(
        &self,
        filter: &Filter,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = || {
            div()
                .mb_1()
                .text_12()
                .text_color(gray(600))
                .child(filter.label.clone())
        };
        match self.controls.get(&filter.fieldname) {
            Some(FilterControl::Input(state)) => {
                let focused = state.focus_handle(cx).is_focused(window);
                // `Date.vue`: the formatted date and a calendar until clicked.
                let shown = match (&filter.kind, focused, self.report.get(&filter.fieldname)) {
                    (FilterKind::Date, false, FilterValue::Date(date)) => {
                        Some(self.session.borrow().formatter.date(date))
                    }
                    (FilterKind::Date, false, _) => Some(String::new()),
                    _ => None,
                };
                let overlay = shown.clone().map(|text| {
                    let state = state.clone();
                    let empty = text.is_empty();
                    div()
                        .id(SharedString::from(format!(
                            "filter-{}-shown",
                            filter.fieldname
                        )))
                        .absolute()
                        .inset_0()
                        .h_flex()
                        .px_2()
                        .cursor_text()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .when(empty, |d| d.text_color(gray(500)))
                                .child(if empty { filter.label.clone() } else { text }),
                        )
                        .child(books::icon("calendar", px(16.), gray(600)))
                        .on_click(move |_, window, cx| state.focus_handle(cx).focus(window, cx))
                });
                div()
                    .v_flex()
                    .child(label())
                    .child(
                        control_frame(false, focused)
                            .relative()
                            .min_h(px(30.))
                            .child(
                                Input::new(state)
                                    .appearance(false)
                                    .h(px(28.))
                                    .px_2()
                                    .text_13()
                                    .when(shown.is_some(), |i| i.text_color(hsla(0., 0., 0., 0.))),
                            )
                            .children(overlay),
                    )
                    .into_any_element()
            }
            Some(FilterControl::Link(state)) => div()
                .v_flex()
                .child(label())
                .child(state.clone())
                .into_any_element(),
            Some(FilterControl::Check) | None => {
                let checked = self.report.get(&filter.fieldname) == FilterValue::Bool(true);
                let fieldname = filter.fieldname.clone();
                // `Check.vue` with a border, at the foot of its cell (`self-end`).
                div()
                    .id(SharedString::from(format!("filter-{fieldname}")))
                    .mt_auto()
                    .h_flex()
                    .min_h(px(30.))
                    .px_2()
                    .rounded(books::RADIUS)
                    .border_1()
                    .border_color(gray(200))
                    .bg(gray(25))
                    .cursor_pointer()
                    .child(check_box(checked))
                    .child(
                        div()
                            .ml_3()
                            .text_13()
                            .text_color(gray(600))
                            .child(filter.label.clone()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_filter(&fieldname, FilterValue::Bool(!checked), window, cx)
                    }))
                    .into_any_element()
            }
        }
    }

    /// `usePagination`: the ledgers and GST returns come a page at a time.
    fn paginated(&self) -> bool {
        matches!(
            self.report.report_name(),
            "general-ledger" | "stock-ledger" | "gstr-1" | "gstr-2"
        )
    }

    /// The rows shown: those not under a folded group, and on the page.
    fn visible_rows(&self) -> Vec<(usize, ReportRow)> {
        let mut hidden_below: Option<usize> = None;
        let mut rows = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let level = row.level.unwrap_or(0);
            if let Some(fold) = hidden_below {
                if level > fold {
                    continue;
                }
                hidden_below = None;
            }
            if row.is_group && self.folded.contains(&i) {
                hidden_below = Some(level);
            }
            rows.push((i, row.clone()));
        }
        if self.paginated() {
            let start = ((self.page - 1) * self.count).min(rows.len());
            let end = (start + self.count).min(rows.len());
            rows = rows[start..end].to_vec();
        }
        rows
    }

    /// `Paginator.vue`.
    fn render_paginator(&self, cx: &mut Context<Self>) -> Div {
        let total = self.rows.len();
        let pages = total.div_ceil(self.count.max(1)).max(1);
        let first = ((self.page - 1) * self.count + 1).min(total);
        let last = (self.page * self.count).min(total);
        let page = self.page;
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
                        gray(600).opacity(0.)
                    },
                ))
        };
        let counts: Vec<i64> = [50i64, 100, 500, -1]
            .into_iter()
            .filter(|&c| {
                if c == -1 {
                    total >= 50
                } else {
                    total >= c as usize
                }
            })
            .collect();
        div()
            .h_flex()
            .flex_none()
            .h(px(50.))
            .px_4()
            .text_12()
            .text_color(gray(800))
            .child(div().flex_1().child(format!("{first} - {last}")))
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .justify_center()
                    .gap_1()
                    .child(
                        arrow("report-page-back", "chevron-left", page > 1).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.page = page.saturating_sub(1).max(1);
                                cx.notify();
                            },
                        )),
                    )
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
                        arrow("report-page-next", "chevron-right", page < pages).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.page = (page + 1).min(pages);
                                cx.notify();
                            }),
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
                            div()
                                .id(SharedString::from(format!("report-count-{c}")))
                                .w_9()
                                .h_flex()
                                .justify_center()
                                .cursor_pointer()
                                .when(self.count == size, |d| {
                                    d.bg(gray(100)).rounded(books::RADIUS)
                                })
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

/// A cell as `ListReport.vue`'s `getCellStyle` places it: `width × 8rem`, `px-3`, none
/// before the first or after the last.
fn cell_box(column: &Column, first: bool, last: bool) -> Div {
    let d = div()
        .w(px(column_width(column)))
        .flex_none()
        .px_3()
        .when(first, |d| d.pl_0())
        .when(last, |d| d.pr_0())
        .whitespace_nowrap()
        .overflow_hidden();
    match column.align {
        Align::Right => d.h_flex().justify_end(),
        Align::Center => d.h_flex().justify_center(),
        Align::Left => d.h_flex(),
    }
}

/// `cell.width ?? 1`, in pixels.
fn column_width(column: &Column) -> f32 {
    let width = if column.width > 0. { column.width } else { 1. };
    width as f32 * WCONST
}

/// `getCellColorClass`: red or green when asked, gray for nothing or zero.
fn cell_color(cell: &latte_core::reports::ReportCell, precision: usize) -> Hsla {
    use latte_core::reports::RawValue;
    match cell.color {
        Some(CellColor::Red) => return books::color("red", 600),
        Some(CellColor::Green) => return books::color("green", 600),
        None => {}
    }
    let zero = match &cell.raw_value {
        RawValue::Empty => true,
        RawValue::Text(text) => text.is_empty(),
        RawValue::Bool(b) => !b,
        RawValue::Number(n) => format!("{n:.precision$}").parse::<f64>().unwrap_or(0.) == 0.,
        RawValue::Money(m) => m.round(Some(precision as i64)).parse::<f64>().unwrap_or(0.) == 0.,
        RawValue::Date(_) => false,
    };
    if zero { gray(600) } else { gray(900) }
}

impl Render for ReportPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `Report.vue`'s header: the Export group (`getCommonExportActions`).
        let json = self.report.has_json_export();
        let menu = self.menu.then(|| {
            deferred(
                anchored().anchor(Anchor::TopRight).snap_to_window().child(
                    popover_panel()
                        .id("export-menu")
                        .occlude()
                        .mt_2()
                        .min_w(px(160.))
                        .p_1()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = false;
                            cx.notify();
                        }))
                        .child(dropdown_item("export-csv", "CSV").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.menu = false;
                                this.export(false, window, cx)
                            },
                        )))
                        .when(json, |d| {
                            d.child(dropdown_item("export-json", "JSON").mt_1().on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.menu = false;
                                    this.export(true, window, cx)
                                }),
                            ))
                        }),
                ),
            )
            .with_priority(1)
        });
        let header = page_header(t(self.report.title()), cx).child(
            div()
                .relative()
                .child(
                    Button::new("export")
                        .label(t("Export"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu = !this.menu;
                            cx.notify();
                        })),
                )
                .children(menu),
        );
        let filters: Vec<AnyElement> = self
            .filters
            .clone()
            .iter()
            .map(|f| self.render_filter(f, window, cx))
            .collect();
        let columns = self.columns.clone();
        let n = columns.len();
        let width: f32 = columns.iter().map(column_width).sum::<f32>() + 32.;
        let column_header = div()
            .h_flex()
            .min_w(px(width))
            .w_full()
            .h_12()
            .flex_none()
            .px_4()
            .border_b_1()
            .border_color(gray(200))
            .text_13()
            .children(
                columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| cell_box(c, i == 0, i + 1 == n).child(c.label.clone())),
            );
        let rows = self.visible_rows();
        let count = rows.len();
        let precision = self.session.borrow().formatter.display_precision;
        let body = uniform_list(
            "report-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|r| {
                        let (i, row) = &rows[r];
                        let i = *i;
                        let is_group = row.is_group;
                        div()
                            .id(r)
                            .h_flex()
                            .min_w(px(width))
                            .w_full()
                            .h_12()
                            .px_4()
                            .text_13()
                            .when(r + 1 < count, |d| d.border_b_1().border_color(gray(200)))
                            .when(is_group, |d| {
                                d.cursor_pointer()
                                    .hover(|d| d.bg(gray(50)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.folded.remove(&i) {
                                            this.folded.insert(i);
                                        }
                                        cx.notify();
                                    }))
                            })
                            .children(row.cells.iter().zip(columns.iter()).enumerate().map(
                                |(c, (cell, column))| {
                                    let mut d = cell_box(column, c == 0, c + 1 == n)
                                        .text_color(cell_color(cell, precision));
                                    if cell.indent > 0 {
                                        d = d.pl(px(32. * cell.indent as f32));
                                    }
                                    if cell.bold {
                                        d = d.font_weight(FontWeight::BOLD);
                                    }
                                    if cell.italics {
                                        d = d.italic();
                                    }
                                    d.child(cell.value.clone())
                                },
                            ))
                    })
                    .collect()
            }),
        )
        .flex_1();
        let empty = self.rows.is_empty();

        let toast = self.info.clone().map(|message| {
            toast(
                message,
                ToastKind::Success,
                cx.listener(|this, _, _, cx| {
                    this.info = None;
                    cx.notify();
                }),
                window,
            )
        });

        div()
            .v_flex()
            .size_full()
            .text_color(gray(900))
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .when(!self.filters.is_empty(), |d| {
                d.child(
                    div()
                        .grid()
                        .grid_cols(5)
                        .gap_4()
                        .p_4()
                        .flex_none()
                        .border_b_1()
                        .border_color(gray(200))
                        .children(filters),
                )
            })
            .map(|d| {
                if empty {
                    d.child(
                        div()
                            .w_full()
                            .mt_20()
                            .h_flex()
                            .justify_center()
                            .text_13()
                            .text_color(gray(800))
                            .child(t("No Values to be Displayed")),
                    )
                } else {
                    d.child(
                        div()
                            .id("report-body")
                            .flex_1()
                            .min_h_0()
                            .v_flex()
                            .overflow_x_scroll()
                            .child(column_header)
                            .child(body),
                    )
                }
            })
            .when(self.paginated() && !empty, |d| {
                d.child(self.render_paginator(cx))
            })
            .when(!self.paginated(), |d| d.child(div().h_4().flex_none()))
            .children(toast)
    }
}
