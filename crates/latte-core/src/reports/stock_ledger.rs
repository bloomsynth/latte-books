// Port of reports/inventory/StockLedger.ts @ v0.36.0: each stock ledger entry with the
// item's balance at its location after it, valued first in, first out.
//
// Books bug Latte fixes: the To Date is included. Books compares each entry's date and
// time with midnight at the start of the To Date, so the entries of the To Date itself are
// left out.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};
use latte_db::QueryFilter;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::models::helpers::single_is_truthy;
use crate::reports::stock::{
    ComputedStockLedgerEntry, ValuationMethod, get_raw_stock_ledger_entries,
    get_stock_ledger_entries,
};
use crate::reports::{
    Align, CellColor, Column, Filter, FilterKind, FilterValue, RawValue, Report, ReportCell,
    ReportRow,
};

#[derive(Debug, Clone)]
pub struct StockLedger {
    pub item: Option<String>,
    pub location: Option<String>,
    pub batch: Option<String>,
    pub from_date: Option<Date>,
    pub to_date: Option<Date>,
    pub ascending: bool,
    /// `All`, or a schema name.
    pub reference_type: String,
    pub reference_name: Option<String>,
    /// `none`, `item`, `location` or `referenceName`.
    pub group_by: String,
}

impl Default for StockLedger {
    fn default() -> Self {
        Self {
            item: None,
            location: None,
            batch: None,
            from_date: None,
            to_date: None,
            ascending: false,
            reference_type: "All".to_owned(),
            reference_name: None,
            group_by: "none".to_owned(),
        }
    }
}

/// `hasBatches`.
pub(crate) fn has_batches(fyo: &Fyo) -> bool {
    single_is_truthy(fyo, "InventorySettings", "enableBatches")
}

/// `hasSerialNumbers`.
fn has_serial_numbers(fyo: &Fyo) -> bool {
    single_is_truthy(fyo, "InventorySettings", "enableSerialNumber")
}

/// The start of a date, as Books' `Date.parse` reads a date: midnight UTC.
fn start_of(date: Date) -> Option<Timestamp> {
    date.to_zoned(TimeZone::UTC)
        .ok()
        .map(|zoned| zoned.timestamp())
}

/// Whether an entry is before the From Date.
pub(crate) fn is_before(date: Option<Timestamp>, from_date: Option<Date>) -> bool {
    match (date, from_date.and_then(start_of)) {
        (Some(date), Some(from)) => date < from,
        _ => false,
    }
}

/// Whether an entry is after the To Date, which is included.
pub(crate) fn is_after(date: Option<Timestamp>, to_date: Option<Date>) -> bool {
    let end = to_date
        .and_then(|date| date.checked_add(1.day()).ok())
        .and_then(start_of);
    match (date, end) {
        (Some(date), Some(end)) => date >= end,
        _ => false,
    }
}

/// `_setRawData`: every entry with its balance.
pub(crate) fn computed_entries(fyo: &Fyo) -> Result<Vec<ComputedStockLedgerEntry>, Error> {
    let raw = get_raw_stock_ledger_entries(fyo, QueryFilter::new())?;
    Ok(get_stock_ledger_entries(&raw, ValuationMethod::Fifo))
}

pub(crate) fn text_filter(value: &FilterValue) -> Option<String> {
    match value {
        FilterValue::Text(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

pub(crate) fn date_filter(value: &FilterValue) -> Option<Date> {
    match value {
        FilterValue::Date(date) => Some(*date),
        _ => None,
    }
}

pub(crate) fn options(options: &[(&str, &str)]) -> Vec<(String, String)> {
    options
        .iter()
        .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
        .collect()
}

/// The kind of value a column shows, as its `fieldtype`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Int,
    Datetime,
    Text,
    Float,
    Currency,
}

/// A column and the kind of its values.
pub(crate) fn column(label: &str, fieldname: &str, kind: Kind, width: f64) -> (Column, Kind) {
    let align = match kind {
        Kind::Int | Kind::Float | Kind::Currency => Align::Right,
        Kind::Datetime | Kind::Text => Align::Left,
    };
    (Column::new(label, fieldname, align, width), kind)
}

/// A cell's raw value before it's shown.
pub(crate) enum Value {
    Number(f64),
    Text(String),
    Date(Option<Timestamp>),
}

/// `_convertRawDataRowToReportRow`: `colour` gives a column's fixed colour, or `None` to
/// colour it by its sign.
pub(crate) fn report_row(
    fyo: &Fyo,
    formatter: &Formatter,
    columns: &[(Column, Kind)],
    value: impl Fn(&str) -> Value,
    colour: impl Fn(&str) -> Option<Option<CellColor>>,
) -> Result<ReportRow, Error> {
    let mut cells = Vec::new();
    for (column, kind) in columns {
        let fieldname = column.fieldname.as_str();
        let (text, raw_value, number) = match (value(fieldname), kind) {
            (Value::Number(number), Kind::Currency) => {
                let money = fyo.pesa().from_f64(number)?;
                (
                    formatter.currency(&money),
                    RawValue::Money(money),
                    Some(number),
                )
            }
            (Value::Number(number), Kind::Int) => {
                (number.to_string(), RawValue::Number(number), Some(number))
            }
            (Value::Number(number), _) => (
                formatter.float(number),
                RawValue::Number(number),
                Some(number),
            ),
            (Value::Date(Some(date)), _) => (formatter.datetime(date), RawValue::Date(date), None),
            (Value::Date(None), _) => (String::new(), RawValue::Empty, None),
            (Value::Text(text), _) if fieldname == "referenceType" => {
                let label = fyo
                    .schema(&text)
                    .map_or_else(|| text.clone(), |schema| schema.label.clone());
                (label, RawValue::Text(text), None)
            }
            (Value::Text(text), _) => (text.clone(), RawValue::Text(text), None),
        };

        let color = match colour(fieldname) {
            Some(Some(color)) => Some(color),
            Some(None) => match number {
                Some(number) if number > 0.0 => Some(CellColor::Green),
                Some(number) if number < 0.0 => Some(CellColor::Red),
                _ => None,
            },
            None => None,
        };
        cells.push(ReportCell {
            value: text,
            raw_value,
            align: column.align,
            width: column.width,
            color,
            ..ReportCell::default()
        });
    }
    Ok(ReportRow {
        cells,
        ..ReportRow::default()
    })
}

/// An empty row between groups.
pub(crate) fn empty_row(columns: &[(Column, Kind)]) -> ReportRow {
    ReportRow {
        is_empty: true,
        cells: columns
            .iter()
            .map(|(c, _)| ReportCell {
                width: c.width,
                ..ReportCell::default()
            })
            .collect(),
        ..ReportRow::default()
    }
}

impl StockLedger {
    /// `setDefaultFilters`: from a year ago to tomorrow.
    pub fn set_default_filters(&mut self, today: Date) {
        if self.to_date.is_none() {
            self.to_date = today.checked_add(1.day()).ok();
            self.from_date = today.checked_sub(1.year()).ok();
        }
    }

    /// `_getFilteredRawData`: the entries the filters let through, numbered, newest first
    /// unless in ascending order.
    fn filtered(&self, mut raw: Vec<ComputedStockLedgerEntry>) -> Vec<ComputedStockLedgerEntry> {
        if !self.ascending {
            raw.reverse();
        }

        let mut filtered = Vec::new();
        for mut row in raw {
            let is = |filter: &Option<String>, value: &str| {
                filter.as_deref().is_none_or(|filter| filter == value)
            };
            if !is(&self.item, &row.item)
                || !is(&self.location, &row.location)
                || !is(&self.batch, &row.batch)
                || is_after(row.date, self.to_date)
                || is_before(row.date, self.from_date)
                || (self.reference_type != "All" && row.reference_type != self.reference_type)
                || !is(&self.reference_name, &row.reference_name)
            {
                continue;
            }
            row.name = filtered.len() as i64 + 1;
            filtered.push(row);
        }
        filtered
    }

    /// `_getGroupedRawData`: the entries of each item, location or reference together,
    /// numbered again, with `None` between groups.
    fn grouped(&self, raw: Vec<ComputedStockLedgerEntry>) -> Vec<Option<ComputedStockLedgerEntry>> {
        let key = |row: &ComputedStockLedgerEntry| match self.group_by.as_str() {
            "item" => row.item.clone(),
            "location" => row.location.clone(),
            "referenceName" => row.reference_name.clone(),
            _ => String::new(),
        };
        if !matches!(
            self.group_by.as_str(),
            "item" | "location" | "referenceName"
        ) {
            return raw.into_iter().map(Some).collect();
        }

        let mut groups: indexmap::IndexMap<String, Vec<ComputedStockLedgerEntry>> =
            indexmap::IndexMap::new();
        for row in raw {
            groups.entry(key(&row)).or_default().push(row);
        }
        let mut grouped = Vec::new();
        let mut index = 0;
        for rows in groups.into_values() {
            for mut row in rows {
                index += 1;
                row.name = index;
                grouped.push(Some(row));
            }
            grouped.push(None);
        }
        grouped.pop();
        grouped
    }

    /// `getColumns`, with the kind of each.
    fn kinded_columns(&self, fyo: &Fyo) -> Vec<(Column, Kind)> {
        let mut columns = vec![
            column("#", "name", Kind::Int, 0.5),
            column("Date", "date", Kind::Datetime, 1.25),
            column("Item", "item", Kind::Text, 1.0),
            column("Location", "location", Kind::Text, 1.0),
        ];
        if has_batches(fyo) {
            columns.push(column("Batch", "batch", Kind::Text, 1.0));
        }
        if has_serial_numbers(fyo) {
            columns.push(column("Serial Number", "serialNumber", Kind::Text, 1.0));
        }
        columns.extend([
            column("Quantity", "quantity", Kind::Float, 1.0),
            column("Balance Qty.", "balanceQuantity", Kind::Float, 1.0),
            column("Incoming rate", "incomingRate", Kind::Currency, 1.0),
            column("Valuation Rate", "valuationRate", Kind::Currency, 1.0),
            column("Balance Value", "balanceValue", Kind::Currency, 1.0),
            column("Value Change", "valueChange", Kind::Currency, 1.0),
            column("Ref. Name", "referenceName", Kind::Text, 1.0),
            column("Ref. Type", "referenceType", Kind::Text, 1.0),
        ]);
        columns
    }
}

fn entry_value(row: &ComputedStockLedgerEntry, fieldname: &str) -> Value {
    match fieldname {
        "name" => Value::Number(row.name as f64),
        "date" => Value::Date(row.date),
        "item" => Value::Text(row.item.clone()),
        "location" => Value::Text(row.location.clone()),
        "batch" => Value::Text(row.batch.clone()),
        "serialNumber" => Value::Text(row.serial_number.clone()),
        "quantity" => Value::Number(row.quantity),
        "balanceQuantity" => Value::Number(row.balance_quantity),
        "incomingRate" => Value::Number(row.incoming_rate),
        "valuationRate" => Value::Number(row.valuation_rate),
        "balanceValue" => Value::Number(row.balance_value),
        "valueChange" => Value::Number(row.value_change),
        "referenceName" => Value::Text(row.reference_name.clone()),
        "referenceType" => Value::Text(row.reference_type.clone()),
        _ => Value::Text(String::new()),
    }
}

impl Report for StockLedger {
    fn title(&self) -> &'static str {
        "Stock Ledger"
    }

    fn report_name(&self) -> &'static str {
        "stock-ledger"
    }

    fn filters(&self, fyo: &Fyo) -> Vec<Filter> {
        let mut filters = vec![
            Filter::new(
                "referenceType",
                "Ref Type",
                FilterKind::Select(options(&[
                    ("All", "All"),
                    ("Stock Movements", "StockMovement"),
                    ("Shipment", "Shipment"),
                    ("Purchase Receipt", "PurchaseReceipt"),
                ])),
            ),
            Filter::new(
                "referenceName",
                "Ref Name",
                FilterKind::DynamicLink {
                    references: "referenceType".to_owned(),
                },
            ),
            Filter::new("item", "Item", FilterKind::Link("Item".to_owned())),
            Filter::new(
                "location",
                "Location",
                FilterKind::Link("Location".to_owned()),
            ),
        ];
        if has_batches(fyo) {
            filters.push(Filter::new(
                "batch",
                "Batch",
                FilterKind::Link("Batch".to_owned()),
            ));
        }
        filters.extend([
            Filter::new("fromDate", "From Date", FilterKind::Date),
            Filter::new("toDate", "To Date", FilterKind::Date),
            Filter::new(
                "groupBy",
                "Group By",
                FilterKind::Select(options(&[
                    ("None", "none"),
                    ("Item", "item"),
                    ("Location", "location"),
                    ("Reference", "referenceName"),
                ])),
            ),
            Filter::new("ascending", "Ascending Order", FilterKind::Check),
        ]);
        filters
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "referenceType" => FilterValue::Text(self.reference_type.clone()),
            "referenceName" => FilterValue::text(self.reference_name.as_deref()),
            "item" => FilterValue::text(self.item.as_deref()),
            "location" => FilterValue::text(self.location.as_deref()),
            "batch" => FilterValue::text(self.batch.as_deref()),
            "fromDate" => FilterValue::date(self.from_date),
            "toDate" => FilterValue::date(self.to_date),
            "groupBy" => FilterValue::Text(self.group_by.clone()),
            "ascending" => FilterValue::Bool(self.ascending),
            _ => FilterValue::Null,
        }
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        match fieldname {
            "referenceType" => {
                self.reference_type = text_filter(&value).unwrap_or_else(|| "All".to_owned());
            }
            "referenceName" => self.reference_name = text_filter(&value),
            "item" => self.item = text_filter(&value),
            "location" => self.location = text_filter(&value),
            "batch" => self.batch = text_filter(&value),
            "fromDate" => self.from_date = date_filter(&value),
            "toDate" => self.to_date = date_filter(&value),
            "groupBy" => self.group_by = text_filter(&value).unwrap_or_else(|| "none".to_owned()),
            "ascending" => self.ascending = matches!(value, FilterValue::Bool(true)),
            _ => {}
        }
    }

    fn columns(&self, fyo: &Fyo, _formatter: &Formatter) -> Result<Vec<Column>, Error> {
        Ok(self
            .kinded_columns(fyo)
            .into_iter()
            .map(|(c, _)| c)
            .collect())
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.set_default_filters(Zoned::now().date());
        let columns = self.kinded_columns(fyo);
        let filtered = self.filtered(computed_entries(fyo)?);
        let colour =
            |fieldname: &str| matches!(fieldname, "quantity" | "valueChange").then_some(None);
        self.grouped(filtered)
            .iter()
            .map(|row| match row {
                Some(row) => report_row(fyo, formatter, &columns, |f| entry_value(row, f), colour),
                None => Ok(empty_row(&columns)),
            })
            .collect()
    }
}
