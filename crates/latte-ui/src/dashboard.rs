//! The dashboard: `src/pages/Dashboard/` @ v0.36.0.
//!
//! The data comes from `latte_core::dashboard`; this draws it.
//!
//! As in Books, each chart has its own period, and the header's period sets them all;
//! picking another period on a chart leaves the header at "Set Period".
//! The Paid and Unpaid amounts open those invoices.

use gpui_kit::component::chart::{AreaChart, BarChart, PieChart};
use gpui_kit::component::*;
use gpui_kit::*;

use latte_core::dashboard::{self, CashflowMonth, Period, Unpaid};

use gpui_kit::prelude::FluentBuilder as _;

use crate::books::{self, Text as _, gray, white};
use crate::i18n::t;
use crate::session::Shared;
use crate::widgets::{dropdown_item, error_message, page_header, popover_panel};
use crate::{Nav, Navigate};
use latte_core::views::sidebar::Route;

/// A dashboard widget, each with its own period (`BaseDashboardChart.vue`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Widget {
    Header,
    Cashflow,
    Sales,
    Purchases,
    Profit,
    Expenses,
}

impl Widget {
    const CHARTS: [Widget; 5] = [
        Widget::Cashflow,
        Widget::Sales,
        Widget::Purchases,
        Widget::Profit,
        Widget::Expenses,
    ];

    /// The periods a widget offers: the charts by month have no This Month.
    fn options(self) -> &'static [Period] {
        match self {
            Widget::Cashflow | Widget::Profit => {
                &[Period::ThisYear, Period::ThisQuarter, Period::Ytd]
            }
            _ => &[
                Period::ThisYear,
                Period::ThisQuarter,
                Period::ThisMonth,
                Period::Ytd,
            ],
        }
    }

    fn id(self) -> &'static str {
        match self {
            Widget::Header => "header",
            Widget::Cashflow => "cashflow",
            Widget::Sales => "sales",
            Widget::Purchases => "purchases",
            Widget::Profit => "profit",
            Widget::Expenses => "expenses",
        }
    }
}

/// `PeriodSelector.vue`'s labels.
fn period_label(period: Option<Period>) -> &'static str {
    match period {
        None => "Set Period",
        Some(Period::Ytd) => "Year to Date",
        Some(period) => period.label(),
    }
}

pub struct DashboardPage {
    session: Shared,
    /// The header's period; `None` once a chart's own differs ("Set Period").
    period: Option<Period>,
    /// Each chart's period, in `Widget::CHARTS` order.
    periods: [Period; 5],
    /// The period menu open, if any.
    menu: Option<Widget>,
    /// The invoices bar under the pointer, which shows its counts.
    hovered: Option<&'static str>,
    cashflow: Vec<CashflowMonth>,
    has_cashflow: bool,
    sales: Option<Unpaid>,
    purchases: Option<Unpaid>,
    profit: Vec<(String, f64)>,
    has_profit: bool,
    expenses: Vec<(String, f64)>,
    error: Option<String>,
}

impl EventEmitter<Navigate> for DashboardPage {}

fn today() -> jiff::civil::Date {
    jiff::Zoned::now().date()
}

impl DashboardPage {
    pub fn new(session: Shared, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            session,
            period: Some(Period::ThisYear),
            periods: [Period::ThisYear; 5],
            menu: None,
            hovered: None,
            cashflow: Vec::new(),
            has_cashflow: false,
            sales: None,
            purchases: None,
            profit: Vec::new(),
            has_profit: false,
            expenses: Vec::new(),
            error: None,
        };
        page.load(cx);
        page
    }

    fn period_of(&self, widget: Widget) -> Period {
        Widget::CHARTS
            .iter()
            .position(|w| *w == widget)
            .map_or(Period::ThisYear, |i| self.periods[i])
    }

    /// `handlePeriodChange`: the header's period sets every chart that offers it; a
    /// chart's own leaves the header at "Set Period" when it differs.
    fn set_period(&mut self, widget: Widget, period: Period, cx: &mut Context<Self>) {
        self.menu = None;
        if widget == Widget::Header {
            self.period = Some(period);
            for (i, chart) in Widget::CHARTS.iter().enumerate() {
                if chart.options().contains(&period) {
                    self.periods[i] = period;
                }
            }
        } else {
            if let Some(i) = Widget::CHARTS.iter().position(|w| *w == widget) {
                self.periods[i] = period;
            }
            if self.period != Some(period) {
                self.period = None;
            }
        }
        self.load(cx);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let result = self.read();
        self.error = result.err().map(|e| e.to_string());
        cx.notify();
    }

    fn read(&mut self) -> Result<(), latte_core::Error> {
        let s = self.session.borrow();
        let fyo = &s.fyo;
        let periods: Vec<(Widget, Period)> = Widget::CHARTS
            .iter()
            .map(|w| (*w, self.period_of(*w)))
            .collect();
        let dates = |widget: Widget| {
            let period = periods
                .iter()
                .find(|(w, _)| *w == widget)
                .map_or(Period::ThisYear, |(_, p)| *p);
            dashboard::dates_and_months(period, today())
        };
        self.has_cashflow = dashboard::has_cashflow(fyo)?;
        self.cashflow = dashboard::cashflow(fyo, &dates(Widget::Cashflow))?;
        self.sales = Some(dashboard::unpaid(
            fyo,
            "SalesInvoice",
            &dates(Widget::Sales),
        )?);
        self.purchases = Some(dashboard::unpaid(
            fyo,
            "PurchaseInvoice",
            &dates(Widget::Purchases),
        )?);
        (self.profit, self.has_profit) = dashboard::profit_and_loss(fyo, &dates(Widget::Profit))?;
        self.expenses = dashboard::top_expenses(fyo, &dates(Widget::Expenses))?;
        Ok(())
    }

    fn money(&self, value: f64) -> String {
        let s = self.session.borrow();
        match s.fyo.pesa().from_f64(value) {
            Ok(money) => s.formatter.currency(&money),
            Err(_) => value.to_string(),
        }
    }

    /// `PeriodSelector.vue`: the period and a chevron, opening a menu of the periods.
    fn period_selector(&self, widget: Widget, cx: &mut Context<Self>) -> Div {
        let current = if widget == Widget::Header {
            self.period
        } else {
            Some(self.period_of(widget))
        };
        let open = self.menu == Some(widget);
        let menu = open.then(|| {
            deferred(
                anchored().anchor(Anchor::TopRight).snap_to_window().child(
                    popover_panel()
                        .id(SharedString::from(format!("period-menu-{}", widget.id())))
                        .occlude()
                        .mt_2()
                        .min_w(px(160.))
                        .p_1()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .children(widget.options().iter().enumerate().map(|(i, &period)| {
                            dropdown_item(
                                SharedString::from(format!("{}-{i}", widget.id())),
                                t(period_label(Some(period))),
                            )
                            .when(i > 0, |d| d.mt_1())
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.set_period(widget, period, cx),
                            ))
                        })),
                ),
            )
            .with_priority(1)
        });
        let color = if current.is_some() {
            gray(900)
        } else {
            gray(600)
        };
        div()
            .relative()
            .child(
                div()
                    .id(SharedString::from(format!("period-{}", widget.id())))
                    .h_flex()
                    .py_1()
                    .text_12()
                    .text_color(color)
                    .cursor_pointer()
                    .hover(|d| d.text_color(gray(800)))
                    .child(t(period_label(current)))
                    .child(books::icon("chevron-down", px(12.), color).ml_1())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = if this.menu == Some(widget) {
                            None
                        } else {
                            Some(widget)
                        };
                        cx.notify();
                    })),
            )
            .children(menu)
    }

    /// `SectionHeader.vue`: the title, then the widget's period.
    fn section_header(&self, title: &str, widget: Widget, cx: &mut Context<Self>) -> Div {
        div()
            .h_flex()
            .justify_between()
            .child(
                div()
                    .text_13()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t(title)),
            )
            .child(self.period_selector(widget, cx))
    }

    fn empty(text: &str) -> Div {
        div()
            .my_20()
            .h_flex()
            .justify_center()
            .text_13()
            .text_color(gray(600))
            .child(t(text))
    }

    fn render_cashflow(&self, cx: &mut Context<Self>) -> Div {
        let (inflow, outflow) = (books::color("blue", 500), books::color("pink", 500));
        // `Cashflow.vue`: the title, the legend and the period on one row.
        let header = div()
            .h_flex()
            .justify_between()
            .child(
                div()
                    .text_13()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Cashflow")),
            )
            .child(
                div()
                    .h_flex()
                    .gap_8()
                    .text_13()
                    .child(legend(t("Inflow"), inflow))
                    .child(legend(t("Outflow"), outflow)),
            )
            .child(self.period_selector(Widget::Cashflow, cx));
        let section = div().v_flex().p_4().child(header);
        if !self.has_cashflow {
            return section.child(Self::empty("No transactions yet"));
        }
        let money = self.money_fn();
        // Four round steps up to the largest month, so the axis reads 0, 20K, 40K….
        let top = self
            .cashflow
            .iter()
            .map(|m| m.inflow.max(m.outflow))
            .fold(0.0, f64::max);
        let step = round_step(top / 4.);
        let chart = AreaChart::new(self.cashflow.clone())
            .x(|m: &CashflowMonth| SharedString::from(month_label(&m.yearmonth)))
            .y(|m: &CashflowMonth| m.inflow)
            .stroke(inflow)
            .fill(fade(inflow))
            .natural()
            .y(|m: &CashflowMonth| m.outflow)
            .stroke(outflow)
            .fill(fade(outflow))
            .natural()
            .y_domain(0., step * 4.)
            .y_padding(0., 0.)
            .y_axis(true)
            .y_tick_format(short_number)
            .tooltip_value(move |_, _, value| SharedString::from(money(value)));
        section.child(div().mt_4().h(px(220.)).child(chart))
    }

    /// `UnpaidInvoices.vue`: what was paid and what wasn't, and a bar; its counts show
    /// over the bar.
    fn render_unpaid(
        &self,
        schema_name: &'static str,
        unpaid: Option<&Unpaid>,
        cx: &mut Context<Self>,
    ) -> Div {
        let (title, color, widget) = if schema_name == "SalesInvoice" {
            ("Sales Invoices", "blue", Widget::Sales)
        } else {
            ("Purchase Invoices", "pink", Widget::Purchases)
        };
        let (paid_color, unpaid_color) = (books::color(color, 500), books::color(color, 200));
        let empty = Unpaid {
            total: 0.,
            unpaid: 0.,
            paid: 0.,
            count: 0,
            paid_count: 0,
            unpaid_count: 0,
        };
        let unpaid = unpaid.unwrap_or(&empty);
        let has_data = unpaid.count > 0;
        let paid_share = if unpaid.total > 0.0 {
            (unpaid.paid / unpaid.total).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let header = self.section_header(title, widget, cx);
        let amount = |value: f64, label: &'static str, count: usize| {
            let schema_label = if schema_name == "SalesInvoice" {
                "Sales Invoice"
            } else {
                "Purchase Invoice"
            };
            // `routeToInvoices`: the paid or unpaid invoices of the kind.
            let (operator, title) = if label == "Paid" {
                ("=", format!("{} {schema_label}", t("Paid")))
            } else {
                ("!=", format!("{} {schema_label}", t("Unpaid")))
            };
            div()
                .id(SharedString::from(format!("{schema_name}-{label}")))
                .when(count > 0, |d| {
                    d.cursor_pointer().on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(Navigate(Nav::Route(Route::filtered_list(
                            schema_name,
                            &title,
                            &[("outstandingAmount", operator, "0")],
                        ))))
                    }))
                })
                .text_12()
                .font_weight(FontWeight::MEDIUM)
                .when(!has_data, |d| {
                    d.bg(gray(200)).text_color(gray(200)).rounded(books::RADIUS)
                })
                .child(self.money(value))
                .child(
                    div()
                        .ml_1()
                        .when(has_data, |d| {
                            d.text_color(gray(900)).font_weight(FontWeight::NORMAL)
                        })
                        .child(t(label)),
                )
                .h_flex()
        };
        let paid = amount(unpaid.paid, "Paid", unpaid.paid_count);
        let unpaid_amount = amount(unpaid.unpaid, "Unpaid", unpaid.unpaid_count);
        let hovered = self.hovered == Some(schema_name);
        let counts = (hovered && has_data).then(|| {
            div()
                .absolute()
                .bottom(px(24.))
                .left(relative(0.4))
                .v_flex()
                .px_2()
                .py_1()
                .bg(white())
                .shadow_md()
                .border_l_4()
                .border_color(paid_color)
                .text_12()
                .text_color(gray(900))
                .child(
                    div()
                        .h_flex()
                        .gap_4()
                        .justify_between()
                        .child(t("Paid"))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(unpaid.paid_count.to_string()),
                        ),
                )
                .when(unpaid.unpaid_count > 0, |d| {
                    d.child(
                        div()
                            .h_flex()
                            .gap_4()
                            .justify_between()
                            .child(t("Unpaid"))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(unpaid.unpaid_count.to_string()),
                            ),
                    )
                })
        });
        div()
            .v_flex()
            .flex_1()
            .p_4()
            .child(header)
            .child(
                div()
                    .mt_4()
                    .h_flex()
                    .justify_between()
                    .child(paid)
                    .child(unpaid_amount),
            )
            .child(
                div()
                    .id(SharedString::from(format!("bar-{schema_name}")))
                    .relative()
                    .mt_2()
                    .h_4()
                    .w_full()
                    .rounded(books::RADIUS)
                    .bg(if has_data { unpaid_color } else { gray(200) })
                    .when(has_data, |d| {
                        d.child(
                            div()
                                .h_full()
                                .w(relative(paid_share))
                                .rounded_l(books::RADIUS)
                                .bg(paid_color),
                        )
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.hovered = hovered.then_some(schema_name);
                        cx.notify();
                    }))
                    .children(counts),
            )
    }

    fn render_profit(&self, cx: &mut Context<Self>) -> Div {
        let section =
            div()
                .v_flex()
                .p_4()
                .child(self.section_header("Profit and Loss", Widget::Profit, cx));
        if !self.has_profit {
            return section.child(Self::empty("No transactions yet"));
        }
        let money = self.money_fn();
        let chart = BarChart::new(self.profit.clone())
            .band(|(month, _): &(String, f64)| SharedString::from(month_label(month)))
            .value(|(_, value): &(String, f64)| *value)
            .fill(|(_, value): &(String, f64), _, _, _| {
                books::color(if *value < 0.0 { "pink" } else { "blue" }, 500)
            })
            // The value axis spans the data's own range, so its ticks fall on odd
            // amounts; the grid and the tooltips are enough.
            .grid(true)
            .tick_margin(2)
            .max_band_width(px(18.))
            .corner_radii(px(3.))
            .min_length(2.)
            .tooltip_value(move |_, value| SharedString::from(money(value)));
        section.child(div().mt_4().h(px(220.)).child(chart))
    }

    fn render_expenses(&self, cx: &mut Context<Self>) -> Div {
        let section =
            div()
                .v_flex()
                .p_4()
                .child(self.section_header("Top Expenses", Widget::Expenses, cx));
        if self.expenses.is_empty() {
            return section.child(Self::empty("No expenses in this period"));
        }
        let total: f64 = self.expenses.iter().map(|(_, v)| *v).sum();
        let slices: Vec<(String, f64, Hsla)> = self
            .expenses
            .iter()
            .enumerate()
            .map(|(i, (account, value))| (account.clone(), *value, expense_color(i)))
            .collect();
        let money = self.money_fn();
        let donut = PieChart::new(slices.clone())
            .value(|(_, value, _): &(String, f64, Hsla)| *value as f32)
            .color(|(_, _, color): &(String, f64, Hsla)| *color)
            .inner_radius(62.)
            .outer_radius(88.)
            .pad_angle(0.02)
            .tooltip_name(|(account, _, _): &(String, f64, Hsla)| account.clone().into())
            .tooltip_value(move |_, value, _| SharedString::from(money(f64::from(value))));
        // `Expenses.vue`'s legend: a square, the account and its total.
        let legend = div()
            .v_flex()
            .w_1_2()
            .gap_4()
            .justify_center()
            .children(slices.iter().map(|(account, value, color)| {
                div()
                    .h_flex()
                    .text_12()
                    .child(div().size_3().flex_none().rounded(px(2.)).bg(*color))
                    .child(div().ml_2().w(px(112.)).truncate().child(account.clone()))
                    .child(div().ml_auto().flex_none().child(self.money(*value)))
            }));
        let center = div()
            .absolute()
            .inset_0()
            .v_flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.money(total)),
            )
            .child(
                div()
                    .text_11()
                    .text_color(hex_a1abb4())
                    .child(t("Total Spending")),
            );
        section.child(
            div().mt_4().h_flex().items_center().child(legend).child(
                div().w_1_2().h_flex().justify_center().child(
                    div()
                        .relative()
                        .flex_none()
                        .size(px(190.))
                        .child(donut)
                        .child(center),
                ),
            ),
        )
    }

    /// Formats an amount as money, for the charts' tooltips.
    fn money_fn(&self) -> impl Fn(f64) -> String + 'static {
        let session = self.session.clone();
        move |value| {
            let s = session.borrow();
            match s.fyo.pesa().from_f64(value) {
                Ok(money) => s.formatter.currency(&money),
                Err(_) => value.to_string(),
            }
        }
    }
}

/// The donut's label colour (`DonutChart.vue`).
fn hex_a1abb4() -> Hsla {
    books::hex(0xA1ABB4)
}

/// A colour fading to clear downwards, under a line in an area chart.
fn fade(color: Hsla) -> Background {
    linear_gradient(
        180.,
        linear_color_stop(color.opacity(0.35), 0.),
        linear_color_stop(color.opacity(0.), 1.),
    )
}

/// The top expenses' colours (`Expenses.vue`): pink 500 down to 100.
fn expense_color(i: usize) -> Hsla {
    books::color("pink", [500, 400, 300, 200, 100][i.min(4)])
}

/// The smallest of 1, 2, 2.5 or 5 times a power of ten that is at least `value`.
fn round_step(value: f64) -> f64 {
    if value <= 0. {
        return 1.;
    }
    let power = 10f64.powf(value.log10().floor());
    [1., 2., 2.5, 5., 10.]
        .into_iter()
        .map(|m| m * power)
        .find(|step| *step >= value)
        .unwrap_or(10. * power)
}

/// An axis label: `44K`, `1.2M`.
fn short_number(value: f64) -> String {
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (value / 1e9, "B")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "K")
    } else {
        (value, "")
    };
    let text = format!("{scaled:.1}");
    format!("{}{suffix}", text.trim_end_matches(".0"))
}

/// `yyyy-MM` as Books labels it: `Oct 2026`.
fn month_label(yearmonth: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = yearmonth.split('-');
    match (
        parts.next(),
        parts.next().and_then(|m| m.parse::<usize>().ok()),
    ) {
        (Some(year), Some(month)) if (1..=12).contains(&month) => {
            format!("{} {year}", MONTHS[month - 1])
        }
        _ => yearmonth.to_owned(),
    }
}

fn legend(label: String, color: Hsla) -> Div {
    div()
        .h_flex()
        .gap_2()
        .child(div().size_3().rounded(px(2.)).bg(color))
        .child(div().text_color(gray(900)).child(label))
}

impl Render for DashboardPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `Dashboard.vue`: the page's period in a bordered box.
        let header = page_header(t("Dashboard"), cx).child(
            div()
                .h_flex()
                .px_3()
                .border_1()
                .border_color(gray(200))
                .rounded(books::RADIUS)
                .bg(gray(50))
                .child(self.period_selector(Widget::Header, cx)),
        );
        let divider = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        div()
            .v_flex()
            .size_full()
            .text_color(gray(900))
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(
                div()
                    .id("dashboard")
                    .v_flex()
                    .flex_1()
                    .overflow_y_scroll()
                    .child(self.render_cashflow(cx))
                    .child(divider())
                    .child(
                        div()
                            .h_flex()
                            .items_stretch()
                            .child(
                                div()
                                    .flex_1()
                                    .h_flex()
                                    .border_r_1()
                                    .border_color(gray(200))
                                    .child(self.render_unpaid(
                                        "SalesInvoice",
                                        self.sales.as_ref(),
                                        cx,
                                    )),
                            )
                            .child(div().flex_1().h_flex().child(self.render_unpaid(
                                "PurchaseInvoice",
                                self.purchases.as_ref(),
                                cx,
                            ))),
                    )
                    .child(divider())
                    .child(
                        div()
                            .h_flex()
                            .items_stretch()
                            .child(
                                div()
                                    .flex_1()
                                    .border_r_1()
                                    .border_color(gray(200))
                                    .child(self.render_profit(cx)),
                            )
                            .child(div().flex_1().child(self.render_expenses(cx))),
                    )
                    .child(divider()),
            )
    }
}
