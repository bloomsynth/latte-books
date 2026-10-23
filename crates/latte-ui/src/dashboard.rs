//! The dashboard: `src/pages/Dashboard/` @ v0.36.0.
//!
//! The data comes from `latte_core::dashboard`; this draws it.
//!
//! Differences from Books:
//! - The period is picked in the header for the whole page; Books also has a picker on
//!   each chart.
//! - The charts are plain bars: Cashflow shows each month's in and out side by side,
//!   Profit and Loss each month's profit (or loss, in red), and Expenses the top five as
//!   bars rather than a donut.

use gpui_kit::component::button::*;
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

    /// Bars for each month, side by side when there are two series.
    fn month_bars(months: &[(String, Vec<(f64, Hsla)>)], cx: &App) -> Div {
        let theme = cx.theme();
        let max = months
            .iter()
            .flat_map(|(_, bars)| bars.iter().map(|(v, _)| v.abs()))
            .fold(0.0_f64, f64::max)
            .max(1.0);
        div()
            .h_flex()
            .items_end()
            .gap_2()
            .h(px(160.))
            .children(months.iter().map(|(month, bars)| {
                div()
                    .v_flex()
                    .flex_1()
                    .h_full()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .h_flex()
                            .items_end()
                            .justify_center()
                            .gap_0p5()
                            .children(bars.iter().map(|(value, color)| {
                                let height = (value.abs() / max * 130.0) as f32;
                                div()
                                    .w(px(10.))
                                    .h(px(height.max(1.0)))
                                    .rounded_sm()
                                    .bg(*color)
                            })),
                    )
                    .child(
                        div()
                            .h_flex()
                            .justify_center()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(month_label(month)),
                    )
            }))
    }

    fn render_cashflow(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let section = Self::section("Cashflow", cx);
        if !self.has_cashflow {
            return section.child(Self::empty("No transactions yet", cx));
        }
        let months: Vec<_> = self
            .cashflow
            .iter()
            .map(|m| {
                (
                    m.yearmonth.clone(),
                    vec![(m.inflow, theme.blue), (m.outflow, theme.red.opacity(0.6))],
                )
            })
            .collect();
        section
            .child(
                div()
                    .h_flex()
                    .gap_4()
                    .text_sm()
                    .child(legend(t("Inflow"), theme.blue))
                    .child(legend(t("Outflow"), theme.red.opacity(0.6))),
            )
            .child(Self::month_bars(&months, cx))
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
        let theme = cx.theme().clone();
        let section = Self::section("Profit and Loss", cx);
        if !self.has_profit {
            return section.child(Self::empty("No transactions yet", cx));
        }
        let months: Vec<_> = self
            .profit
            .iter()
            .map(|(month, value)| {
                let color = if *value < 0.0 { theme.red } else { theme.green };
                (month.clone(), vec![(*value, color)])
            })
            .collect();
        section.child(Self::month_bars(&months, cx))
    }

    fn render_expenses(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let section = Self::section("Top Expenses", cx);
        if self.expenses.is_empty() {
            return section.child(Self::empty("No expenses in this period", cx));
        }
        let max = self
            .expenses
            .iter()
            .map(|(_, v)| *v)
            .fold(0.0, f64::max)
            .max(1.0);
        let total: f64 = self.expenses.iter().map(|(_, v)| *v).sum();
        let colors = [
            theme.blue,
            theme.cyan,
            theme.green,
            theme.yellow,
            theme.magenta,
        ];
        section
            .children(
                self.expenses
                    .iter()
                    .enumerate()
                    .map(|(i, (account, value))| {
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .h_flex()
                                    .justify_between()
                                    .text_sm()
                                    .child(account.clone())
                                    .child(self.money(*value)),
                            )
                            .child(
                                div()
                                    .h(px(8.))
                                    .w_full()
                                    .rounded_full()
                                    .bg(theme.muted)
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative((value / max) as f32))
                                            .rounded_full()
                                            .bg(colors[i % colors.len()]),
                                    ),
                            )
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} {}", t("Total Spending"), self.money(total))),
            )
    }
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
