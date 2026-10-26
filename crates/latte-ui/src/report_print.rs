//! A report's Print View: `src/pages/PrintView/ReportPrintView.vue` @ v0.36.0.
//!
//! The report as it was last shown (Books keeps its reports in `fyo.store.reports`),
//! with the rows from Start From Row Index, the Number of Rows, the page size, Is
//! Landscape and the picked columns on the right. Save as PDF and Print.

use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use latte_core::reports::{self, Align, Column, ReportRow};
use latte_db::DocValue;
use latte_print::report::{ReportColumn, ReportPrint, print_report};
use latte_print::templates::PRINT_SIZES;

use crate::Navigate;
use crate::books::{self, Text as _, gray, white};
use crate::i18n::t;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::print_parts::{self, Laid};
use crate::session::Shared;
use crate::widgets::{Button, check_box, error_message, field_label, page_header, small_input};

/// A report as its page last showed it, for its Print View.
#[derive(Clone)]
pub struct ReportSnapshot {
    pub name: String,
    pub title: String,
    pub columns: Vec<Column>,
    pub rows: Vec<ReportRow>,
}

/// The report last shown, as Books' `fyo.store.reports` keeps it.
pub struct LastReport(pub ReportSnapshot);

impl Global for LastReport {}

/// `var(--w-quick-edit)`: the settings panel's width.
const PANEL_WIDTH: f32 = 22. * 16.;

pub struct ReportPrintPage {
    session: Shared,
    report: ReportSnapshot,
    start: Entity<InputState>,
    limit: Entity<InputState>,
    size: Entity<LinkInput>,
    print_size: &'static str,
    landscape: bool,
    columns: Vec<bool>,
    printout: Option<latte_print::Printout>,
    pages: Vec<(Arc<Image>, (f64, f64))>,
    error: Option<String>,
    generation: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for ReportPrintPage {}

impl ReportPrintPage {
    /// The report `name` as last shown, or run now with its default filters.
    pub fn snapshot(session: &Shared, name: &str, cx: &App) -> Result<ReportSnapshot, String> {
        if let Some(LastReport(last)) = cx.try_global::<LastReport>()
            && last.name == name
        {
            return Ok(last.clone());
        }
        let mut report = reports::get_report(name).ok_or_else(|| format!("No report {name}"))?;
        let s = session.borrow();
        let rows = report
            .run(&s.fyo, &s.formatter)
            .map_err(|e| e.to_string())?;
        let columns = report
            .columns(&s.fyo, &s.formatter)
            .map_err(|e| e.to_string())?;
        Ok(ReportSnapshot {
            name: name.to_owned(),
            title: report.title().to_owned(),
            columns,
            rows,
        })
    }

    pub fn new(
        session: Shared,
        report: ReportSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let count = report.rows.len();
        let start = cx.new(|cx| InputState::new(window, cx).default_value("1"));
        let limit = cx.new(|cx| InputState::new(window, cx).default_value(count.to_string()));
        let options = PRINT_SIZES
            .iter()
            .map(|(n, _, _)| LinkOption {
                value: (*n).into(),
                label: (*n).into(),
            })
            .collect();
        let size = cx.new(|cx| LinkInput::new(options, t("Print Size"), window, cx));
        size.update(cx, |p, cx| p.set_value(Some("A4".into()), window, cx));
        let mut subscriptions = Vec::new();
        for input in [&start, &limit] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this: &mut Self, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.render_report(cx);
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &size,
            window,
            |this: &mut Self, _, event: &LinkPicked, _, cx| {
                if let Some((name, _, _)) =
                    PRINT_SIZES.iter().find(|(n, _, _)| *n == event.0.as_ref())
                {
                    this.print_size = name;
                    this.render_report(cx);
                }
            },
        ));
        let columns = vec![true; report.columns.len()];
        let mut page = Self {
            session,
            report,
            start,
            limit,
            size,
            print_size: "A4",
            landscape: false,
            columns,
            printout: None,
            pages: Vec::new(),
            error: None,
            generation: 0,
            _subscriptions: subscriptions,
        };
        page.render_report(cx);
        page
    }

    /// `size`: the page, turned for Is Landscape.
    fn page_size(&self) -> (f64, f64) {
        let (_, w, h) = PRINT_SIZES
            .iter()
            .find(|(n, _, _)| *n == self.print_size)
            .copied()
            .unwrap_or(("A4", 21.0, 29.7));
        let (long, short) = (w.max(h), w.min(h));
        if self.landscape {
            (long, short)
        } else {
            (short, long)
        }
    }

    /// `matrix`: the picked columns of the rows from Start, Number of Rows of them.
    fn print_data(&self, cx: &App) -> ReportPrint {
        let number = |input: &Entity<InputState>, default: usize| {
            input
                .read(cx)
                .value()
                .trim()
                .parse::<usize>()
                .unwrap_or(default)
        };
        let total = self.report.rows.len();
        let start = number(&self.start, 1).saturating_sub(1);
        let limit = number(&self.limit, total);
        let end = (start + limit).min(total);
        let picked: Vec<usize> = (0..self.report.columns.len())
            .filter(|&i| self.columns[i])
            .collect();
        let columns = picked
            .iter()
            .map(|&i| {
                let column = &self.report.columns[i];
                ReportColumn {
                    label: column.label.clone(),
                    align: match column.align {
                        Align::Right => "right",
                        Align::Center => "center",
                        Align::Left => "left",
                    },
                }
            })
            .collect();
        let rows = self.report.rows[start.min(end)..end]
            .iter()
            .map(|row| {
                picked
                    .iter()
                    .map(|&i| {
                        row.cells
                            .get(i)
                            .map(|c| c.value.clone())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect();
        let s = self.session.borrow();
        let company_name = match s.fyo.get_value("PrintSettings", None, "companyName") {
            Some(DocValue::Text(name)) => name,
            _ => String::new(),
        };
        let (width, height) = self.page_size();
        ReportPrint {
            company_name,
            title: t(&self.report.title),
            columns,
            rows,
            printed: s.formatter.datetime(jiff::Timestamp::now()),
            width,
            height,
        }
    }

    fn render_report(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let data = self.print_data(cx);
        let task = print_parts::lay_out(move || print_report(&data), cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(Laid { printout, pages }) => {
                        this.printout = Some(printout);
                        this.pages = pages;
                        this.error = None;
                    }
                    Err(errors) => {
                        this.error = Some(latte_print::errors_text(&errors));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// `savePDF`: named "<title> - <date>".
    fn save_pdf(&mut self, print: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(printout) = &self.printout else {
            return;
        };
        let name = {
            let s = self.session.borrow();
            let today = jiff::Zoned::now().date();
            format!("{} - {}", t(&self.report.title), s.formatter.date(today))
        };
        print_parts::output(printout.pdf(), print, &name, window, cx);
    }
}

impl Render for ReportPrintPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = format!("{} {}", t("Print"), t(&self.report.title));
        let ready = self.printout.is_some();
        let header = page_header(title, cx).child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    Button::new("report-save-pdf")
                        .primary()
                        .label(t("Save as PDF"))
                        .disabled(!ready)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_pdf(false, window, cx)),
                        ),
                )
                .child(
                    Button::new("report-print")
                        .primary()
                        .label(t("Print"))
                        .disabled(!ready)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_pdf(true, window, cx)),
                        ),
                ),
        );
        // `setScale`.
        let available = f32::from(window.viewport_size().width) - 26. * 16. - 224.;
        let width = self.page_size().0 as f32 * 37.2;
        let scale = (available / width).clamp(0.1, 1.);
        let check = |id: SharedString, label: SharedString, checked: bool| {
            div()
                .id(id)
                .h_flex()
                .gap_3()
                .px_2()
                .min_h(px(30.))
                .cursor_pointer()
                .child(check_box(checked))
                .child(div().text_13().text_color(gray(600)).child(label))
        };
        let landscape = check(
            "report-landscape".into(),
            t("Is Landscape").into(),
            self.landscape,
        )
        .rounded(books::RADIUS)
        .border_1()
        .border_color(gray(200))
        .bg(gray(25))
        .on_click(cx.listener(|this, _, _, cx| {
            this.landscape = !this.landscape;
            this.render_report(cx);
        }));
        let columns = self.report.columns.iter().enumerate().map(|(i, column)| {
            check(
                SharedString::from(format!("report-column-{i}")),
                column.label.clone().into(),
                self.columns[i],
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.columns[i] = !this.columns[i];
                this.render_report(cx);
            }))
        });
        let section = || div().p_4().border_t_1().border_color(gray(200));
        let panel = div()
            .id("report-print-settings")
            .w(px(PANEL_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .v_flex()
            .border_l_1()
            .border_color(gray(200))
            .bg(white())
            .child(div().p_4().text_12().text_color(gray(600)).child(format!(
                "{} {}",
                t("Hidden values will be visible on Print on."),
                t("Report will use more than one page if required.")
            )))
            .child(
                section()
                    .child(field_label(t("Start From Row Index"), false, cx))
                    .child(small_input(&self.start))
                    .child(
                        div()
                            .mt_4()
                            .child(field_label(t("Number of Rows"), false, cx)),
                    )
                    .child(small_input(&self.limit)),
            )
            .child(
                section()
                    .child(field_label(t("Print Size"), false, cx))
                    .child(self.size.clone())
                    .child(div().mt_4().child(landscape)),
            )
            .child(
                section()
                    .child(
                        div()
                            .text_12()
                            .text_color(gray(600))
                            .child(t("Pick Columns")),
                    )
                    .child(
                        div()
                            .mt_1()
                            .grid()
                            .grid_cols(2)
                            .rounded(books::RADIUS)
                            .border_1()
                            .border_color(gray(200))
                            .children(columns),
                    ),
            );
        div()
            .v_flex()
            .size_full()
            .text_13()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .h_flex()
                    .child(
                        div()
                            .id("report-print-pages")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_scroll()
                            .p_4()
                            .bg(gray(25))
                            .child(print_parts::pages_view(
                                &self.pages,
                                self.printout.as_ref().map_or(0, |p| p.page_count()),
                                scale,
                            )),
                    )
                    .child(panel),
            )
    }
}
