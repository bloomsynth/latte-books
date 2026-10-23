// Port of reports/inventory/StockBalance.ts @ v0.36.0: each item's opening, incoming,
// outgoing and closing stock at each location, for a period.
//
// Books bug Latte fixes: the To Date is included, as in the Stock Ledger.

use jiff::civil::Date;
use jiff::{ToSpan, Zoned};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::reports::stock::{StockBalanceEntry, StockBalanceFilters, get_stock_balance_entries};
use crate::reports::stock_ledger::{
    Kind, Value, column, computed_entries, date_filter, has_batches, is_after, is_before,
    report_row, text_filter,
};
use crate::reports::{CellColor, Column, Filter, FilterKind, FilterValue, Report, ReportRow};

#[derive(Debug, Clone, Default)]
pub struct StockBalance {
    pub item: Option<String>,
    pub location: Option<String>,
    pub batch: Option<String>,
    pub from_date: Option<Date>,
    pub to_date: Option<Date>,
}

impl StockBalance {
    /// `setDefaultFilters`: from a year ago to tomorrow.
    pub fn set_default_filters(&mut self, today: Date) {
        if self.to_date.is_none() {
            self.to_date = today.checked_add(1.day()).ok();
            self.from_date = today.checked_sub(1.year()).ok();
        }
    }

    /// The report's entries, each numbered.
    pub fn entries(&self, fyo: &Fyo) -> Result<Vec<StockBalanceEntry>, Error> {
        let from_date = self.from_date;
        let to_date = self.to_date;
        let before = move |date| is_before(date, from_date);
        let after = move |date| is_after(date, to_date);
        let filters = StockBalanceFilters {
            item: self.item.as_deref(),
            location: self.location.as_deref(),
            batch: self.batch.as_deref(),
            before: &before,
            after: &after,
        };
        let mut entries = get_stock_balance_entries(&computed_entries(fyo)?, &filters);
        for (idx, entry) in entries.iter_mut().enumerate() {
            entry.name = idx as i64 + 1;
        }
        Ok(entries)
    }

    fn kinded_columns(&self, fyo: &Fyo) -> Vec<(Column, Kind)> {
        let mut columns = vec![
            column("#", "name", Kind::Int, 0.5),
            column("Item", "item", Kind::Text, 1.0),
            column("Location", "location", Kind::Text, 1.0),
        ];
        if has_batches(fyo) {
            columns.push(column("Batch", "batch", Kind::Text, 1.0));
        }
        // As in Books, the balance and opening values are shown as numbers, not amounts.
        columns.extend([
            column("Balance Qty.", "balanceQuantity", Kind::Float, 1.0),
            column("Balance Value", "balanceValue", Kind::Float, 1.0),
            column("Opening Qty.", "openingQuantity", Kind::Float, 1.0),
            column("Opening Value", "openingValue", Kind::Float, 1.0),
            column("In Qty.", "incomingQuantity", Kind::Float, 1.0),
            column("In Value", "incomingValue", Kind::Currency, 1.0),
            column("Out Qty.", "outgoingQuantity", Kind::Float, 1.0),
            column("Out Value", "outgoingValue", Kind::Currency, 1.0),
            column("Valuation rate", "valuationRate", Kind::Currency, 1.0),
        ]);
        columns
    }
}

fn entry_value(row: &StockBalanceEntry, fieldname: &str) -> Value {
    match fieldname {
        "name" => Value::Number(row.name as f64),
        "item" => Value::Text(row.item.clone()),
        "location" => Value::Text(row.location.clone()),
        "batch" => Value::Text(row.batch.clone()),
        "balanceQuantity" => Value::Number(row.balance_quantity),
        "balanceValue" => Value::Number(row.balance_value),
        "openingQuantity" => Value::Number(row.opening_quantity),
        "openingValue" => Value::Number(row.opening_value),
        "incomingQuantity" => Value::Number(row.incoming_quantity),
        "incomingValue" => Value::Number(row.incoming_value),
        "outgoingQuantity" => Value::Number(row.outgoing_quantity),
        "outgoingValue" => Value::Number(row.outgoing_value),
        "valuationRate" => Value::Number(row.valuation_rate),
        _ => Value::Text(String::new()),
    }
}

impl Report for StockBalance {
    fn title(&self) -> &'static str {
        "Stock Balance"
    }

    fn report_name(&self) -> &'static str {
        "stock-balance"
    }

    fn filters(&self, fyo: &Fyo) -> Vec<Filter> {
        let mut filters = vec![
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
        ]);
        filters
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "item" => FilterValue::text(self.item.as_deref()),
            "location" => FilterValue::text(self.location.as_deref()),
            "batch" => FilterValue::text(self.batch.as_deref()),
            "fromDate" => FilterValue::date(self.from_date),
            "toDate" => FilterValue::date(self.to_date),
            _ => FilterValue::Null,
        }
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        match fieldname {
            "item" => self.item = text_filter(&value),
            "location" => self.location = text_filter(&value),
            "batch" => self.batch = text_filter(&value),
            "fromDate" => self.from_date = date_filter(&value),
            "toDate" => self.to_date = date_filter(&value),
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
        let colour = |fieldname: &str| match fieldname {
            "incomingQuantity" => Some(Some(CellColor::Green)),
            "outgoingQuantity" => Some(Some(CellColor::Red)),
            "balanceQuantity" => Some(None),
            _ => None,
        };
        self.entries(fyo)?
            .iter()
            .map(|row| report_row(fyo, formatter, &columns, |f| entry_value(row, f), colour))
            .collect()
    }
}
