// The dashboard's data: getDatesAndPeriodList in src/utils/misc.ts, the dashboard's
// queries in backend/database/bespoke.ts (getCashflow, getTotalOutstanding,
// getIncomeAndExpenses, getTopExpenses) and what the charts in src/pages/Dashboard/ do
// with them @ v0.36.0. Amounts are numbers, as in Books.
//
// Deviation: a period starts at the start of its first day. Books starts it at the time
// of day it is now, so the first day's entries are left out of the cash flow, the
// expenses and the invoices (but not the profit and loss, which compares dates only).

use jiff::ToSpan;
use jiff::civil::Date;
use rusqlite::params;

use crate::Error;
use crate::fyo::Fyo;

/// `PeriodKey`, as the dashboard offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    ThisYear,
    ThisQuarter,
    ThisMonth,
    Ytd,
}

impl Period {
    pub const ALL: [Period; 4] = [
        Period::ThisYear,
        Period::ThisQuarter,
        Period::ThisMonth,
        Period::Ytd,
    ];

    /// Whether the Cashflow and Profit and Loss charts offer the period. This Month has
    /// no months to show, so they keep the period they had.
    pub fn has_months(self) -> bool {
        self != Period::ThisMonth
    }

    pub fn label(self) -> &'static str {
        match self {
            Period::ThisYear => "This Year",
            Period::ThisQuarter => "This Quarter",
            Period::ThisMonth => "This Month",
            Period::Ytd => "YTD",
        }
    }
}

/// `getDatesAndPeriodList`: the period's dates, and its months as `yyyy-MM`.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodDates {
    pub from: Date,
    /// Tomorrow, as in Books.
    pub to: Date,
    pub months: Vec<String>,
}

pub fn dates_and_months(period: Period, today: Date) -> PeriodDates {
    let to = today.tomorrow().unwrap_or(today);
    let from = match period {
        Period::ThisYear => to.checked_sub(12.months()),
        Period::Ytd => Ok(today.first_of_year()),
        Period::ThisQuarter => to.checked_sub(3.months()),
        Period::ThisMonth => Ok(to.first_of_month()),
    }
    .unwrap_or(to);

    // Each month back from `to`, while it isn't before `from`; YTD also takes the one
    // that is. The earliest is then dropped.
    let mut list = vec![to];
    loop {
        let Ok(next) = list[0].checked_sub(1.month()) else {
            break;
        };
        if next < from {
            if period == Period::Ytd {
                list.insert(0, next);
            }
            break;
        }
        list.insert(0, next);
    }
    list.remove(0);
    PeriodDates {
        from,
        to,
        months: list
            .iter()
            .map(|date| date.strftime("%Y-%m").to_string())
            .collect(),
    }
}

fn iso(date: Date) -> String {
    date.strftime("%Y-%m-%d").to_string()
}

/// A month's money in and out of the cash and bank accounts.
#[derive(Debug, Clone, PartialEq)]
pub struct CashflowMonth {
    pub yearmonth: String,
    pub inflow: f64,
    pub outflow: f64,
}

/// The Cashflow chart's data: `getCashflow` for each month of the period.
pub fn cashflow(fyo: &Fyo, dates: &PeriodDates) -> Result<Vec<CashflowMonth>, Error> {
    let conn = fyo.db.core().connection();
    let mut statement = conn.prepare(
        "select strftime('%Y-%m', `date`), sum(cast(`debit` as real)), sum(cast(`credit` as real))
         from `AccountingLedgerEntry`
         where `reverted` = 0
           and `account` in (select `name` from `Account`
                             where `accountType` in ('Cash', 'Bank') and `isGroup` = 0)
           and `date` between ?1 and ?2
         group by 1",
    )?;
    let rows: Vec<(String, f64, f64)> = statement
        .query_map(params![iso(dates.from), iso(dates.to)], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, Option<f64>>(1)?.unwrap_or_default(),
                row.get::<_, Option<f64>>(2)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(dates
        .months
        .iter()
        .map(|month| {
            let row = rows.iter().find(|(m, _, _)| m == month);
            CashflowMonth {
                yearmonth: month.clone(),
                inflow: row.map_or(0.0, |r| r.1),
                outflow: row.map_or(0.0, |r| r.2),
            }
        })
        .collect())
}

/// `setHasData` of the Cashflow chart: whether any cash or bank account has an entry.
pub fn has_cashflow(fyo: &Fyo) -> Result<bool, Error> {
    let count: i64 = fyo.db.core().connection().query_row(
        "select count(*) from `AccountingLedgerEntry`
         where `account` in (select `name` from `Account`
                             where `accountType` in ('Cash', 'Bank'))",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// The UnpaidInvoices chart's data for sales or purchase invoices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unpaid {
    pub total: f64,
    pub unpaid: f64,
    pub paid: f64,
    pub count: usize,
    pub paid_count: usize,
    pub unpaid_count: usize,
}

/// `getTotalOutstanding` and `getCounts` for the submitted invoices of the period.
pub fn unpaid(fyo: &Fyo, schema_name: &str, dates: &PeriodDates) -> Result<Unpaid, Error> {
    if !matches!(schema_name, "SalesInvoice" | "PurchaseInvoice") {
        return Err(Error::Value(format!(
            "{schema_name} has no unpaid invoices"
        )));
    }
    let conn = fyo.db.core().connection();
    let mut statement = conn.prepare(&format!(
        "select cast(`baseGrandTotal` as real), cast(`outstandingAmount` as real)
         from `{schema_name}`
         where `submitted` = 1 and `cancelled` = 0 and `date` between ?1 and ?2"
    ))?;
    let rows: Vec<(f64, f64)> = statement
        .query_map(params![iso(dates.from), iso(dates.to)], |row| {
            Ok((
                row.get::<_, Option<f64>>(0)?.unwrap_or_default(),
                row.get::<_, Option<f64>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<_, _>>()?;
    let total: f64 = rows.iter().map(|r| r.0).sum();
    let unpaid: f64 = rows.iter().map(|r| r.1).sum();
    let unpaid_count = rows.iter().filter(|r| r.1 > 0.0).count();
    Ok(Unpaid {
        total,
        unpaid,
        paid: total - unpaid,
        count: rows.len(),
        paid_count: rows.len() - unpaid_count,
        unpaid_count,
    })
}

fn balances(fyo: &Fyo, root_type: &str, dates: &PeriodDates) -> Result<Vec<(String, f64)>, Error> {
    let sign = if root_type == "Income" {
        "cast(`credit` as real) - cast(`debit` as real)"
    } else {
        "cast(`debit` as real) - cast(`credit` as real)"
    };
    let conn = fyo.db.core().connection();
    let mut statement = conn.prepare(&format!(
        "select strftime('%Y-%m', `date`), sum({sign})
         from `AccountingLedgerEntry`
         where `reverted` = 0 and `date` between date(?1) and date(?2)
           and `account` in (select `name` from `Account` where `rootType` = ?3)
         group by 1"
    ))?;
    let rows = statement
        .query_map(params![iso(dates.from), iso(dates.to), root_type], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// The ProfitAndLoss chart's data: each month's income less its expenses, and whether
/// there was either.
pub fn profit_and_loss(
    fyo: &Fyo,
    dates: &PeriodDates,
) -> Result<(Vec<(String, f64)>, bool), Error> {
    let income = balances(fyo, "Income", dates)?;
    let expense = balances(fyo, "Expense", dates)?;
    let has_data = !income.is_empty() || !expense.is_empty();
    let value = |list: &[(String, f64)], month: &str| {
        list.iter()
            .find(|(m, _)| m == month)
            .map_or(0.0, |(_, v)| *v)
    };
    let months = dates
        .months
        .iter()
        .map(|month| {
            (
                month.clone(),
                value(&income, month) - value(&expense, month),
            )
        })
        .collect();
    Ok((months, has_data))
}

/// The Expenses chart's data: `getTopExpenses`, the five expense accounts most spent
/// on, less those with nothing spent.
pub fn top_expenses(fyo: &Fyo, dates: &PeriodDates) -> Result<Vec<(String, f64)>, Error> {
    let conn = fyo.db.core().connection();
    let mut statement = conn.prepare(
        "select `account`, sum(cast(`debit` as real) - cast(`credit` as real)) as `total`
         from `AccountingLedgerEntry`
         where `reverted` = 0
           and `account` in (select `name` from `Account` where `rootType` = 'Expense')
           and `date` between ?1 and ?2
         group by `account`
         order by `total` desc
         limit 5",
    )?;
    let rows: Vec<(String, f64)> = statement
        .query_map(params![iso(dates.from), iso(dates.to)], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, Option<f64>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows.into_iter().filter(|(_, total)| *total > 0.0).collect())
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use latte_db::DocValue;

    use super::*;
    use crate::setup_tests::{TempFile, indian_company};
    use crate::tests::{map, text};

    #[test]
    fn periods_count_back_in_months_as_books_does() {
        let today = date(2026, 10, 23);
        let year = dates_and_months(Period::ThisYear, today);
        assert_eq!(
            (year.from, year.to),
            (date(2025, 10, 24), date(2026, 10, 24))
        );
        assert_eq!(year.months.len(), 12);
        assert_eq!(year.months[0], "2025-11");
        assert_eq!(year.months[11], "2026-10");

        let ytd = dates_and_months(Period::Ytd, today);
        assert_eq!(ytd.from, date(2026, 1, 1));
        assert_eq!(ytd.months.first().unwrap(), "2026-01");
        assert_eq!(ytd.months.len(), 10);

        assert_eq!(
            dates_and_months(Period::ThisQuarter, today).months,
            vec!["2026-08", "2026-09", "2026-10"]
        );
        let month = dates_and_months(Period::ThisMonth, today);
        assert_eq!(month.from, date(2026, 10, 1));
        // No months, which is why the Cashflow and Profit and Loss charts don't offer it.
        assert!(month.months.is_empty());
    }

    #[test]
    fn the_dashboard_shows_sales_cash_and_expenses() {
        let file = TempFile::new("dashboard");
        let mut fyo = indian_company(&file);
        let values = map([("name", text("Widget")), ("rate", DocValue::Number(100.0))]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let values = map([("name", text("Acme")), ("role", text("Customer"))]);
        fyo.get_new_doc("Party", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();

        let mut names = Vec::new();
        for quantity in [1.0, 2.0] {
            let values = map([("party", text("Acme")), ("date", text("2026-10-01"))]);
            let mut doc = fyo.get_new_doc("SalesInvoice", values).unwrap();
            doc.append(&mut fyo, "items", Default::default()).unwrap();
            doc.set_child(&mut fyo, "items", 0, "item", text("Widget"))
                .unwrap();
            doc.set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(quantity))
                .unwrap();
            doc.sync(&mut fyo).unwrap();
            doc.submit(&mut fyo).unwrap();
            names.push(doc.name().unwrap().to_owned());
        }
        // The first is paid in cash.
        let invoice = fyo.get_doc("SalesInvoice", Some(&names[0])).unwrap();
        let mut payment = crate::models::base_models::invoice::get_payment(&invoice, &fyo)
            .unwrap()
            .unwrap();
        let paid_on = DocValue::Date("2026-10-02T00:00:00Z".parse().unwrap());
        payment.set(&mut fyo, "date", paid_on).unwrap();
        payment.sync(&mut fyo).unwrap();
        payment.submit(&mut fyo).unwrap();

        // Rent, in cash.
        let values = map([
            ("entryType", text("Journal Entry")),
            ("date", text("2026-09-15")),
        ]);
        let mut entry = fyo.get_new_doc("JournalEntry", values).unwrap();
        let money = |n: f64| DocValue::Money(fyo.db.pesa().from_f64(n).unwrap());
        let rows = [
            ("Office Rent", money(50.0), money(0.0)),
            ("Cash", money(0.0), money(50.0)),
        ];
        for (i, (account, debit, credit)) in rows.into_iter().enumerate() {
            entry
                .append(&mut fyo, "accounts", Default::default())
                .unwrap();
            entry
                .set_child(&mut fyo, "accounts", i, "account", text(account))
                .unwrap();
            entry
                .set_child(&mut fyo, "accounts", i, "debit", debit)
                .unwrap();
            entry
                .set_child(&mut fyo, "accounts", i, "credit", credit)
                .unwrap();
        }
        entry.sync(&mut fyo).unwrap();
        entry.submit(&mut fyo).unwrap();

        let dates = dates_and_months(Period::ThisYear, date(2026, 10, 23));
        let sales = unpaid(&fyo, "SalesInvoice", &dates).unwrap();
        assert_eq!(sales.count, 2);
        assert_eq!(sales.paid_count, 1);
        assert_eq!(sales.unpaid_count, 1);
        assert_eq!(sales.total, 300.0);
        assert_eq!(sales.unpaid, 200.0);
        assert_eq!(sales.paid, 100.0);
        assert_eq!(unpaid(&fyo, "PurchaseInvoice", &dates).unwrap().count, 0);

        assert!(has_cashflow(&fyo).unwrap());
        let cashflow = cashflow(&fyo, &dates).unwrap();
        assert_eq!(cashflow.len(), 12);
        let october = cashflow.iter().find(|m| m.yearmonth == "2026-10").unwrap();
        assert_eq!((october.inflow, october.outflow), (100.0, 0.0));
        let september = cashflow.iter().find(|m| m.yearmonth == "2026-09").unwrap();
        assert_eq!((september.inflow, september.outflow), (0.0, 50.0));

        let (months, has_data) = profit_and_loss(&fyo, &dates).unwrap();
        assert!(has_data);
        assert!(months.contains(&("2026-10".to_owned(), 300.0)));
        assert!(months.contains(&("2026-09".to_owned(), -50.0)));

        assert_eq!(
            top_expenses(&fyo, &dates).unwrap(),
            vec![("Office Rent".to_owned(), 50.0)]
        );
    }
}
