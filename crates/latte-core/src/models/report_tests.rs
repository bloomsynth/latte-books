use jiff::civil::{Date, date};
use latte_db::{DocValue, DocValueMap};

use crate::Fyo;
use crate::fyo::format::Formatter;
use crate::models::invoice_tests::invoice;
use crate::models::payment_tests::payments;
use crate::reports::balance_sheet::BalanceSheet;
use crate::reports::general_ledger::GeneralLedger;
use crate::reports::profit_and_loss::ProfitAndLoss;
use crate::reports::trial_balance::TrialBalance;
use crate::reports::{FilterValue, Report, ReportRow, csv};
use crate::tests::{map, row, text};

/// The payment test company with a sales invoice for 3 Widgets (₹354 with GST) on
/// 2025-04-10 and a journal entry paying ₹50 of discounts in cash on 2025-04-01.
fn ledger() -> Fyo {
    let mut fyo = rupees(payments());
    let mut doc = invoice(
        &mut fyo,
        "SalesInvoice",
        "Acme",
        map([("date", april_10())]),
        &[(3.0, DocValueMap::new())],
    );
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();

    let mut entry = fyo
        .get_new_doc(
            "JournalEntry",
            map([
                ("entryType", text("Journal Entry")),
                ("date", text("2025-04-01")),
                (
                    "accounts",
                    DocValue::Table(vec![row("Discounts", 50.0, 0.0), row("Cash", 0.0, 50.0)]),
                ),
            ]),
        )
        .unwrap();
    entry.sync(&mut fyo).unwrap();
    entry.submit(&mut fyo).unwrap();
    fyo
}

/// Gives the rupee its symbol, as a new company's currencies have.
fn rupees(mut fyo: Fyo) -> Fyo {
    let inr = map([("name", text("INR")), ("symbol", text("₹"))]);
    fyo.db.update("Currency", &inr).unwrap();
    fyo
}

/// Each row's cell values, blank rows as empty lists.
fn values(rows: &[ReportRow]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| {
            if row.is_empty {
                Vec::new()
            } else {
                row.cells.iter().map(|cell| cell.value.clone()).collect()
            }
        })
        .collect()
}

fn line(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

/// 2025-04-10 as a date field holds it.
fn april_10() -> DocValue {
    DocValue::Date("2025-04-10T00:00:00Z".parse().unwrap())
}

const APRIL_10: Date = date(2025, 4, 10);

#[test]
fn the_general_ledger_lists_entries_by_voucher_with_a_closing_row() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = GeneralLedger::default();
    report.set("fromDate", FilterValue::Date(date(2025, 4, 1)));
    // The To Date's own entries are included.
    report.set("toDate", FilterValue::Date(APRIL_10));
    report.set("ascending", FilterValue::Bool(true));

    let rows = report.run(&fyo, &formatter).unwrap();
    let rows = values(&rows);
    assert_eq!(rows.len(), 8);
    assert_eq!(
        rows[0],
        line(&[
            "1",
            "Discounts",
            "Apr 1, 2025",
            "₹ 50.00",
            "₹ 0.00",
            "₹ 50.00",
            "",
            "JV-1001",
            "Journal Entry"
        ])
    );
    assert_eq!(
        rows[2],
        line(&[
            "3",
            "Debtors",
            "Apr 10, 2025",
            "₹ 354.00",
            "₹ 0.00",
            "₹ 354.00",
            "Acme",
            "SINV-1001",
            "Sales Invoice"
        ])
    );
    assert_eq!(rows[3][5], "₹ 54.00");
    assert!(rows[6].is_empty());
    assert_eq!(
        rows[7],
        line(&[
            "",
            "Closing",
            "",
            "₹ 404.00",
            "₹ 404.00",
            "₹ 0.00",
            "",
            "",
            ""
        ])
    );
}

#[test]
fn the_general_ledger_totals_each_group() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = GeneralLedger::default();
    report.set("fromDate", FilterValue::Date(date(2025, 4, 1)));
    report.set("toDate", FilterValue::Date(APRIL_10));
    report.set("groupBy", FilterValue::Text("account".to_owned()));
    report.set("account", FilterValue::Text("Cash".to_owned()));

    let rows = values(&report.run(&fyo, &formatter).unwrap());
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1][1], "Total");
    assert_eq!(rows[1][4], "₹ 50.00");
    assert!(rows[2].is_empty());
    assert_eq!(rows[3][5], "₹ -50.00");
}

#[test]
fn profit_and_loss_shows_income_expenses_and_the_profit() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = ProfitAndLoss::default();
    report.set("toDate", FilterValue::Date(date(2025, 4, 30)));
    report.set("count", FilterValue::Int(2));

    let rows = values(&report.run_on(&fyo, &formatter, APRIL_10).unwrap());
    let labels: Vec<_> = report
        .columns(&fyo, &formatter)
        .unwrap()
        .iter()
        .map(|c| c.label.clone())
        .collect();
    assert_eq!(labels, ["Account", "Apr 30, 2025", "Mar 31, 2025"]);
    assert_eq!(
        rows,
        vec![
            line(&["Sales", "₹ 300.00", "₹ 0.00"]),
            line(&["Total Income (Credit)", "₹ 300.00", "₹ 0.00"]),
            vec![],
            line(&["Discounts", "₹ 50.00", "₹ 0.00"]),
            line(&["Total Expense (Debit)", "₹ 50.00", "₹ 0.00"]),
            vec![],
            line(&["Total Profit", "₹ 250.00", "₹ 0.00"]),
        ]
    );
}

#[test]
fn profit_and_loss_with_only_income_shows_it() {
    let mut fyo = rupees(payments());
    let mut doc = invoice(
        &mut fyo,
        "SalesInvoice",
        "Acme",
        map([("date", april_10())]),
        &[(1.0, DocValueMap::new())],
    );
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = ProfitAndLoss::default();
    report.set("toDate", FilterValue::Date(date(2025, 4, 30)));
    report.set("consolidateColumns", FilterValue::Bool(true));

    let rows = values(&report.run_on(&fyo, &formatter, APRIL_10).unwrap());
    assert_eq!(
        rows,
        vec![
            line(&["Sales", "₹ 100.00"]),
            line(&["Total Income (Credit)", "₹ 100.00"]),
        ]
    );
}

#[test]
fn the_balance_sheet_totals_assets_and_liabilities() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = BalanceSheet::default();
    report.set("toDate", FilterValue::Date(date(2025, 4, 30)));
    report.set("consolidateColumns", FilterValue::Bool(true));

    let rows = values(&report.run_on(&fyo, &formatter, APRIL_10).unwrap());
    assert!(rows.contains(&line(&["Debtors", "₹ 354.00"])));
    assert!(rows.contains(&line(&["Cash", "₹ -50.00"])));
    assert!(rows.contains(&line(&["Total Asset (Debit)", "₹ 304.00"])));
    assert!(rows.contains(&line(&["CGST", "₹ 27.00"])));
    assert!(rows.contains(&line(&["Total Liability (Credit)", "₹ 54.00"])));
    assert!(!rows.last().unwrap().is_empty());
}

#[test]
fn the_trial_balance_closes_with_the_opening_plus_the_period() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = TrialBalance::default();
    report.set("fromDate", FilterValue::Date(date(2025, 4, 5)));
    report.set("toDate", FilterValue::Date(APRIL_10));

    let rows = values(&report.run_on(&fyo, &formatter, APRIL_10).unwrap());
    assert!(rows.contains(&line(&[
        "Cash",
        "₹ 0.00",
        "₹ 50.00",
        "₹ 0.00",
        "₹ 0.00",
        "₹ 0.00",
        "₹ 50.00"
    ])));
    assert!(rows.contains(&line(&[
        "Debtors",
        "₹ 0.00",
        "₹ 0.00",
        "₹ 354.00",
        "₹ 0.00",
        "₹ 354.00",
        "₹ 0.00"
    ])));
    assert!(rows.contains(&line(&[
        "Discounts",
        "₹ 50.00",
        "₹ 0.00",
        "₹ 0.00",
        "₹ 0.00",
        "₹ 50.00",
        "₹ 0.00"
    ])));
}

#[test]
fn csv_exports_drop_zero_decimals_and_quote_spaces() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = TrialBalance::default();
    report.set("fromDate", FilterValue::Date(date(2025, 4, 5)));
    report.set("toDate", FilterValue::Date(APRIL_10));
    let rows = report.run_on(&fyo, &formatter, APRIL_10).unwrap();
    let columns = report.columns(&fyo, &formatter).unwrap();

    let exported = csv(&columns, &rows, 2);
    let lines: Vec<_> = exported.split("\r\n").collect();
    assert_eq!(
        lines[0],
        "Account,\"Opening (Dr)\",\"Opening (Cr)\",Debit,Credit,\"Closing (Dr)\",\"Closing (Cr)\""
    );
    assert!(lines.contains(&"Debtors,0,0,354,0,354,0"));
    assert!(csv(&columns, &rows, 0).contains("Debtors,0,0,354,0,354,0"));
}

#[test]
fn json_exports_map_each_row_by_column_label() {
    let fyo = ledger();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut report = TrialBalance::default();
    report.set("fromDate", FilterValue::Date(date(2025, 4, 5)));
    report.set("toDate", FilterValue::Date(APRIL_10));
    let rows = report.run_on(&fyo, &formatter, APRIL_10).unwrap();
    let columns = report.columns(&fyo, &formatter).unwrap();

    let exported: serde_json::Value =
        serde_json::from_str(&crate::reports::json(&report, &fyo, &columns, &rows, 2)).unwrap();
    assert_eq!(exported["reportName"], "trial-balance");
    assert_eq!(exported["softwareName"], "Latte Books");
    assert_eq!(exported["columns"][0]["label"], "Account");
    assert_eq!(exported["filters"]["fromDate"], "2025-04-05");
    assert_eq!(exported["filters"]["toDate"], "2025-04-10");
    let debtors = exported["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["Account"] == "Debtors")
        .unwrap();
    assert_eq!(debtors["Debit"], "354");
    assert!(exported["timestamp"].as_str().unwrap().ends_with('Z'));
}
