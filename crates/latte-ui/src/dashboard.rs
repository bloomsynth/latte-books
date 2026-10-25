//! The dashboard: `src/pages/Dashboard/` @ v0.36.0.
//!
//! The data comes from `latte_core::dashboard`; this draws it.
//!
//! Differences from Books:
//! - The period is picked in the header for the whole page; Books also has a picker on
//!   each chart.

use crate::widgets::Button;
use gpui_kit::component::chart::{AreaChart, BarChart, PieChart};
use gpui_kit::component::*;
use gpui_kit::*;

use latte_core::dashboard::{self, CashflowMonth, Period, Unpaid};
use latte_core::views::sidebar::Route;

use crate::i18n::t;
use crate::session::Shared;
use crate::widgets::{error_message, page_header};
use crate::{Nav, Navigate};

pub struct DashboardPage {
    session: Shared,
    period: Period,
    /// The period of the charts by month, which This Month leaves as it was.
    month_period: Period,
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
            period: Period::ThisYear,
            month_period: Period::ThisYear,
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

    fn set_period(&mut self, period: Period, cx: &mut Context<Self>) {
        self.period = period;
        if period.has_months() {
            self.month_period = period;
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
        let dates = dashboard::dates_and_months(self.period, today());
        let month_dates = dashboard::dates_and_months(self.month_period, today());
        self.has_cashflow = dashboard::has_cashflow(fyo)?;
        self.cashflow = dashboard::cashflow(fyo, &month_dates)?;
        self.sales = Some(dashboard::unpaid(fyo, "SalesInvoice", &dates)?);
        self.purchases = Some(dashboard::unpaid(fyo, "PurchaseInvoice", &dates)?);
        (self.profit, self.has_profit) = dashboard::profit_and_loss(fyo, &month_dates)?;
        self.expenses = dashboard::top_expenses(fyo, &dates)?;
        Ok(())
    }

    fn money(&self, value: f64) -> String {
        let s = self.session.borrow();
        match s.fyo.pesa().from_f64(value) {
            Ok(money) => s.formatter.currency(&money),
            Err(_) => value.to_string(),
        }
    }

    fn period_picker(&self, cx: &mut Context<Self>) -> Div {
        div()
            .h_flex()
            .gap_1()
            .children(Period::ALL.iter().map(|&period| {
                let button = Button::new(SharedString::from(format!("period-{}", period.label())))
                    .small()
                    .label(t(period.label()))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_period(period, cx)));
                if period == self.period {
                    button.primary()
                } else {
                    button.ghost()
                }
            }))
    }

    fn section(title: &str, cx: &App) -> Div {
        let theme = cx.theme();
        div().v_flex().gap_3().p_4().child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .child(t(title)),
        )
    }

    fn empty(text: &str, cx: &App) -> Div {
        div()
            .h(px(120.))
            .h_flex()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(t(text))
    }

    fn render_cashflow(&self, cx: &mut Context<Self>) -> Div {
        let section = Self::section("Cashflow", cx);
        if !self.has_cashflow {
            return section.child(Self::empty("No transactions yet", cx));
        }
        let (inflow, outflow): (Hsla, Hsla) = (INFLOW.into(), OUTFLOW.into());
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
        section
            .child(
                div()
                    .h_flex()
                    .gap_4()
                    .text_sm()
                    .child(legend(t("Inflow"), inflow))
                    .child(legend(t("Outflow"), outflow)),
            )
            .child(div().h(px(220.)).child(chart))
    }

    fn render_unpaid(
        &self,
        schema_name: &'static str,
        unpaid: Option<&Unpaid>,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme().clone();
        let (title, color) = if schema_name == "SalesInvoice" {
            ("Sales Invoices", theme.blue)
        } else {
            ("Purchase Invoices", theme.magenta)
        };
        let new_button = Button::new(SharedString::from(format!("new-{schema_name}")))
            .small()
            .ghost()
            .label(t("New Invoice"))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(Navigate(Nav::Route(Route::Form {
                    schema: schema_name.to_owned(),
                    name: None,
                })))
            }));
        let header = div()
            .h_flex()
            .justify_between()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(t(title)))
            .child(new_button);
        let mut section = div().v_flex().flex_1().gap_3().p_4().child(header);
        let Some(unpaid) = unpaid.filter(|u| u.count > 0) else {
            return section.child(Self::empty("No invoices yet", cx));
        };
        let paid_share = if unpaid.total > 0.0 {
            (unpaid.paid / unpaid.total).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        section = section
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .text_sm()
                    .child(format!("{} {}", self.money(unpaid.paid), t("Paid")))
                    .child(format!("{} {}", self.money(unpaid.unpaid), t("Unpaid"))),
            )
            .child(
                div()
                    .h(px(10.))
                    .w_full()
                    .rounded_full()
                    .bg(color.opacity(0.2))
                    .child(
                        div()
                            .h_full()
                            .w(relative(paid_share))
                            .rounded_full()
                            .bg(color),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} {}", unpaid.paid_count, t("Paid")))
                    .child(format!("{} {}", unpaid.unpaid_count, t("Unpaid"))),
            );
        section
    }

    fn render_profit(&self, cx: &mut Context<Self>) -> Div {
        let section = Self::section("Profit and Loss", cx);
        if !self.has_profit {
            return section.child(Self::empty("No transactions yet", cx));
        }
        let money = self.money_fn();
        let chart = BarChart::new(self.profit.clone())
            .band(|(month, _): &(String, f64)| SharedString::from(month_label(month)))
            .value(|(_, value): &(String, f64)| *value)
            .fill(|(_, value): &(String, f64), _, _, _| {
                let color: Hsla = if *value < 0.0 { OUTFLOW } else { INFLOW }.into();
                color
            })
            // The value axis spans the data's own range, so its ticks fall on odd
            // amounts; the grid and the tooltips are enough.
            .grid(true)
            .tick_margin(2)
            .max_band_width(px(18.))
            .corner_radii(px(3.))
            .min_length(2.)
            .tooltip_value(move |_, value| SharedString::from(money(value)));
        section.child(div().h(px(220.)).child(chart))
    }

    fn render_expenses(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let section = Self::section("Top Expenses", cx);
        if self.expenses.is_empty() {
            return section.child(Self::empty("No expenses in this period", cx));
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
        let legend = div().v_flex().flex_1().gap_3().children(slices.iter().map(
            |(account, value, color)| {
                div()
                    .h_flex()
                    .gap_2()
                    .text_sm()
                    .child(div().size(px(10.)).flex_none().rounded_full().bg(*color))
                    .child(div().flex_1().truncate().child(account.clone()))
                    .child(div().flex_none().child(self.money(*value)))
            },
        ));
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
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t("Total Spending")),
            );
        section.child(
            div().h_flex().gap_6().items_center().child(legend).child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(190.))
                    .child(donut)
                    .child(center),
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

/// Books' chart colours: blue for money in and profit, pink for money out and loss.
const INFLOW: Rgba = Rgba {
    r: 0.231,
    g: 0.510,
    b: 0.965,
    a: 1.0,
};
const OUTFLOW: Rgba = Rgba {
    r: 0.882,
    g: 0.506,
    b: 0.659,
    a: 1.0,
};

/// A colour fading to clear downwards, under a line in an area chart.
fn fade(color: Hsla) -> Background {
    linear_gradient(
        180.,
        linear_color_stop(color.opacity(0.35), 0.),
        linear_color_stop(color.opacity(0.), 1.),
    )
}

/// The top expenses' colours: Books' pink, lighter for each smaller one.
fn expense_color(i: usize) -> Hsla {
    let pink: Hsla = OUTFLOW.into();
    pink.opacity([1.0, 0.75, 0.55, 0.38, 0.24][i.min(4)])
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
        .gap_1()
        .child(div().size(px(10.)).rounded_sm().bg(color))
        .child(label)
}

impl Render for DashboardPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = page_header(t("Dashboard"), cx).child(self.period_picker(cx));
        let divider = || div().w_full().h(px(1.)).bg(theme.border);
        div()
            .v_flex()
            .size_full()
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
                            .items_start()
                            .child(self.render_unpaid("SalesInvoice", self.sales.as_ref(), cx))
                            .child(div().w(px(1.)).h_full().bg(theme.border))
                            .child(self.render_unpaid(
                                "PurchaseInvoice",
                                self.purchases.as_ref(),
                                cx,
                            )),
                    )
                    .child(divider())
                    .child(
                        div()
                            .h_flex()
                            .items_start()
                            .child(div().flex_1().child(self.render_profit(cx)))
                            .child(div().w(px(1.)).h_full().bg(theme.border))
                            .child(div().flex_1().child(self.render_expenses(cx))),
                    ),
            )
    }
}
