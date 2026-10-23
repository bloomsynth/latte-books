// Port of reports/ @ v0.36.0: Report.ts, types.ts and the CSV export of
// commonExporter.ts, with the accounting reports.
//
// Deviations:
// - Books sums report amounts as floats. Latte sums them as money, so a long ledger
//   can't drift by a fraction of a paisa. What is shown is the same.
// - A report is run with `run`, which returns its rows; it doesn't watch the ledger
//   for changes as Books' reports do (`shouldRefresh`). The app runs it again.
// - The JSON export of the accounting reports isn't ported yet. GSTR's is (gstr.rs).
//
// Books bug Latte fixes: exporting to CSV with a display precision of 0 cuts the last
// digit off every amount ("123" becomes "12").

pub mod account_report;
pub mod balance_sheet;
pub mod general_ledger;
pub mod gstr;
pub mod ledger_report;
pub mod profit_and_loss;
pub mod stock;
pub mod stock_balance;
pub mod stock_ledger;
pub mod trial_balance;

use jiff::Timestamp;
use jiff::civil::Date;
use latte_db::pesa::Money;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    Red,
    Green,
}

/// A cell's `rawValue`: the value before it was formatted.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RawValue {
    #[default]
    Empty,
    Text(String),
    Money(Money),
    Number(f64),
    Date(Timestamp),
    Bool(bool),
}

/// `ReportCell`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportCell {
    pub value: String,
    pub raw_value: RawValue,
    pub bold: bool,
    pub italics: bool,
    pub align: Align,
    pub width: f64,
    pub indent: usize,
    pub color: Option<CellColor>,
}

/// `ReportRow`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportRow {
    pub cells: Vec<ReportCell>,
    pub level: Option<usize>,
    pub is_group: bool,
    pub is_empty: bool,
}

/// `ColumnField`.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub label: String,
    pub fieldname: String,
    pub align: Align,
    pub width: f64,
}

impl Column {
    fn new(label: impl Into<String>, fieldname: &str, align: Align, width: f64) -> Self {
        Self {
            label: label.into(),
            fieldname: fieldname.to_owned(),
            align,
            width,
        }
    }
}

/// What a filter is, for the report's filter bar.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterKind {
    /// Options as `(label, value)`.
    Select(Vec<(String, String)>),
    /// A link to a document of this schema.
    Link(String),
    /// A link to a document of the schema named by the filter `references`.
    DynamicLink {
        references: String,
    },
    Date,
    Int {
        min: i64,
    },
    Check,
}

/// A report filter, as Books gives it in `getFilters`.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub fieldname: String,
    pub label: String,
    pub kind: FilterKind,
    pub required: bool,
}

impl Filter {
    fn new(fieldname: &str, label: &str, kind: FilterKind) -> Self {
        Self {
            fieldname: fieldname.to_owned(),
            label: label.to_owned(),
            kind,
            required: false,
        }
    }

    fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// A filter's value.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    Null,
    Text(String),
    Date(Date),
    Int(i64),
    Bool(bool),
}

impl FilterValue {
    fn text(value: Option<&str>) -> Self {
        value.map_or(Self::Null, |value| Self::Text(value.to_owned()))
    }

    fn date(value: Option<Date>) -> Self {
        value.map_or(Self::Null, Self::Date)
    }
}

/// `Report`: a report with its filters, columns and rows.
pub trait Report {
    /// `title`, as shown.
    fn title(&self) -> &'static str;
    /// `reportName`, used in its route and the export's file name.
    fn report_name(&self) -> &'static str;
    /// `getFilters`.
    fn filters(&self, fyo: &Fyo) -> Vec<Filter>;
    /// A filter's value (`report[fieldname]`).
    fn get(&self, fieldname: &str) -> FilterValue;
    /// `set`: sets a filter. A value of the wrong kind is ignored.
    fn set(&mut self, fieldname: &str, value: FilterValue);
    /// `getColumns`.
    fn columns(&self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<Column>, Error>;
    /// `setReportData`: the report's rows for its filters.
    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error>;
    /// Whether the report can be exported now, with the reason it can't.
    fn can_export(&self, _fyo: &Fyo) -> Result<(), Error> {
        Ok(())
    }
    /// Whether the report has a JSON export.
    fn has_json_export(&self) -> bool {
        false
    }
    /// The report's JSON export of its last run, for a report that has one.
    fn export_json(&self, _fyo: &Fyo) -> Option<Result<String, Error>> {
        None
    }
}

/// `getCsvData`: the columns' labels, then a line per row, as RFC 4180 CSV with CRLF line
/// ends. Amounts are written to the display precision, dropping zero decimals.
pub fn csv(columns: &[Column], rows: &[ReportRow], display_precision: usize) -> String {
    let mut matrix: Vec<Vec<String>> = vec![columns.iter().map(|c| c.label.clone()).collect()];
    for row in rows {
        if row.is_empty {
            matrix.push(vec![String::new(); row.cells.len()]);
            continue;
        }
        matrix.push(
            row.cells
                .iter()
                .map(|cell| csv_value(cell, display_precision))
                .collect(),
        );
    }

    matrix
        .iter()
        .map(|row| {
            row.iter()
                .map(|item| csv_item(item))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// `getValueFromCell`.
fn csv_value(cell: &ReportCell, display_precision: usize) -> String {
    match &cell.raw_value {
        RawValue::Date(timestamp) => latte_db::to_iso_string(*timestamp),
        RawValue::Money(money) => {
            let value = money.round(Some(display_precision as i64));
            let zeros = "0".repeat(display_precision);
            match value.split_once('.') {
                Some((whole, fraction)) if display_precision > 0 && fraction == zeros => {
                    whole.to_owned()
                }
                _ => value,
            }
        }
        RawValue::Number(number) => crate::fyo::model::helpers::js_number_string(*number),
        RawValue::Text(text) => text.clone(),
        RawValue::Bool(value) => value.to_string(),
        RawValue::Empty => String::new(),
    }
}

/// `formatStringToCSV`.
pub(crate) fn csv_item(item: &str) -> String {
    let mut item = item;
    let mut should_quote = false;
    if item.len() >= 2 && item.starts_with('"') && item.ends_with('"') {
        should_quote = true;
        item = &item[1..item.len() - 1];
    }
    let mut item = item.to_owned();
    if item.contains('"') {
        should_quote = true;
        item = item.replace('"', "\"\"");
    }
    if item.contains(',') || item.chars().any(char::is_whitespace) {
        should_quote = true;
    }
    if should_quote {
        format!("\"{item}\"")
    } else {
        item
    }
}

/// A report by its `reportName`, with its default filters.
pub fn get_report(report_name: &str) -> Option<Box<dyn Report>> {
    Some(match report_name {
        "general-ledger" => Box::new(general_ledger::GeneralLedger::default()),
        "profit-and-loss" => Box::new(profit_and_loss::ProfitAndLoss::default()),
        "balance-sheet" => Box::new(balance_sheet::BalanceSheet::default()),
        "trial-balance" => Box::new(trial_balance::TrialBalance::default()),
        "stock-ledger" => Box::new(stock_ledger::StockLedger::default()),
        "stock-balance" => Box::new(stock_balance::StockBalance::default()),
        "gstr-1" => Box::new(gstr::Gstr::new(gstr::GstrType::Gstr1)),
        "gstr-2" => Box::new(gstr::Gstr::new(gstr::GstrType::Gstr2)),
        _ => return None,
    })
}

/// The reports Latte has, as `(reportName, title)`, in Books' order.
pub const REPORTS: [(&str, &str); 8] = [
    ("general-ledger", "General Ledger"),
    ("profit-and-loss", "Profit And Loss"),
    ("balance-sheet", "Balance Sheet"),
    ("trial-balance", "Trial Balance"),
    ("stock-ledger", "Stock Ledger"),
    ("stock-balance", "Stock Balance"),
    ("gstr-1", "GSTR1"),
    ("gstr-2", "GSTR2"),
];
