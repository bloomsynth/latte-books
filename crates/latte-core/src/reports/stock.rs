// Port of reports/inventory/helpers.ts @ v0.36.0: the stock ledger entries with their
// running balances and values, the stock balance, and the cost of a shipment.
//
// Books bug Latte fixes: a shipment's cost of goods is worked out for any of its items.
// Books fails on an item with no stock ledger entries when another item has some, as it
// reads the queues of an item it has none for, and on any row without a queue, which it
// goes on to read after adding the row's amount.

use std::collections::HashMap;

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, FilterValue, GetAllOptions, Order, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::models::helpers::{money_or_zero, number, text};
use crate::models::inventory::stock_queue::StockQueue;

/// `RawStockLedgerEntry`.
#[derive(Debug, Clone)]
pub struct RawStockLedgerEntry {
    pub name: String,
    pub date: Option<Timestamp>,
    pub item: String,
    pub batch: Option<String>,
    pub serial_number: Option<String>,
    pub rate: f64,
    pub quantity: f64,
    pub location: String,
    pub reference_name: String,
    pub reference_type: String,
}

/// `ComputedStockLedgerEntry`.
#[derive(Debug, Clone)]
pub struct ComputedStockLedgerEntry {
    pub name: i64,
    pub date: Option<Timestamp>,
    pub item: String,
    pub location: String,
    pub batch: String,
    pub serial_number: String,
    pub quantity: f64,
    pub balance_quantity: f64,
    pub incoming_rate: f64,
    pub valuation_rate: f64,
    pub balance_value: f64,
    pub value_change: f64,
    pub reference_name: String,
    pub reference_type: String,
}

/// `StockBalanceEntry`.
#[derive(Debug, Clone, Default)]
pub struct StockBalanceEntry {
    pub name: i64,
    pub item: String,
    pub location: String,
    pub batch: String,
    pub balance_quantity: f64,
    pub balance_value: f64,
    pub opening_quantity: f64,
    pub opening_value: f64,
    pub incoming_quantity: f64,
    pub incoming_value: f64,
    pub outgoing_quantity: f64,
    pub outgoing_value: f64,
    pub valuation_rate: f64,
}

fn map_text(row: &DocValueMap, fieldname: &str) -> Option<String> {
    match row.get(fieldname) {
        Some(DocValue::Text(text)) if !text.is_empty() => Some(text.clone()),
        Some(DocValue::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

/// `getRawStockLedgerEntries`: oldest first.
pub fn get_raw_stock_ledger_entries(
    fyo: &Fyo,
    filters: QueryFilter,
) -> Result<Vec<RawStockLedgerEntry>, Error> {
    let fields = [
        "name",
        "date",
        "item",
        "batch",
        "serialNumber",
        "rate",
        "quantity",
        "location",
        "referenceName",
        "referenceType",
    ];
    let options = GetAllOptions {
        fields: Some(fields.iter().map(|f| (*f).to_owned()).collect()),
        filters,
        order_by: Some(vec![
            "date".to_owned(),
            "created".to_owned(),
            "name".to_owned(),
        ]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs("StockLedgerEntry", &options)?
        .iter()
        .map(|row| RawStockLedgerEntry {
            name: map_text(row, "name").unwrap_or_default(),
            date: match row.get("date") {
                Some(DocValue::Date(date)) => Some(*date),
                _ => None,
            },
            item: map_text(row, "item").unwrap_or_default(),
            batch: map_text(row, "batch"),
            serial_number: map_text(row, "serialNumber"),
            rate: match row.get("rate") {
                Some(DocValue::Money(rate)) => rate.float(),
                _ => 0.0,
            },
            quantity: match row.get("quantity") {
                Some(DocValue::Number(quantity)) if !quantity.is_nan() => *quantity,
                _ => 0.0,
            },
            location: map_text(row, "location").unwrap_or_default(),
            reference_name: map_text(row, "referenceName").unwrap_or_default(),
            reference_type: map_text(row, "referenceType").unwrap_or_default(),
        })
        .collect())
}

/// `ValuationMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValuationMethod {
    Fifo,
    MovingAverage,
}

/// `getStockLedgerEntries`: each entry with the item's balance at its location and batch
/// after it. Entries whose stock isn't there are left out, as Books leaves them.
pub fn get_stock_ledger_entries(
    raw: &[RawStockLedgerEntry],
    valuation_method: ValuationMethod,
) -> Vec<ComputedStockLedgerEntry> {
    let mut queues: HashMap<(String, String, String), StockQueue> = HashMap::new();
    let mut computed = Vec::new();
    for sle in raw {
        if sle.quantity == 0.0 {
            continue;
        }
        let batch = sle.batch.clone().unwrap_or_default();
        let q = queues
            .entry((sle.item.clone(), sle.location.clone(), batch.clone()))
            .or_default();
        let initial_value = q.value;
        let incoming_rate = if sle.quantity > 0.0 {
            q.inward(sle.rate, sle.quantity)
        } else {
            q.outward(-sle.quantity)
        };
        let Some(incoming_rate) = incoming_rate else {
            continue;
        };

        let valuation_rate = match valuation_method {
            ValuationMethod::Fifo => q.fifo(),
            ValuationMethod::MovingAverage => q.moving_average,
        };
        computed.push(ComputedStockLedgerEntry {
            name: sle.name.parse().unwrap_or(0),
            date: sle.date,
            item: sle.item.clone(),
            location: sle.location.clone(),
            batch,
            serial_number: sle.serial_number.clone().unwrap_or_default(),
            quantity: sle.quantity,
            balance_quantity: q.quantity,
            incoming_rate,
            valuation_rate,
            balance_value: q.value,
            value_change: q.value - initial_value,
            reference_name: sle.reference_name.clone(),
            reference_type: sle.reference_type.clone(),
        });
    }
    computed
}

/// The filters of `getStockBalanceEntries`. The dates are compared as the report's
/// caller decides.
pub struct StockBalanceFilters<'a> {
    pub item: Option<&'a str>,
    pub location: Option<&'a str>,
    pub batch: Option<&'a str>,
    /// Whether an entry is before the period, so counts in the opening balance.
    pub before: &'a dyn Fn(Option<Timestamp>) -> bool,
    /// Whether an entry is after the period, so is left out.
    pub after: &'a dyn Fn(Option<Timestamp>) -> bool,
}

/// `getStockBalanceEntries`: each item's opening, incoming, outgoing and closing stock at
/// each location and batch.
pub fn get_stock_balance_entries(
    computed: &[ComputedStockLedgerEntry],
    filters: &StockBalanceFilters<'_>,
) -> Vec<StockBalanceEntry> {
    let mut map: IndexMap<(String, String, String), StockBalanceEntry> = IndexMap::new();
    for sle in computed {
        if filters.item.is_some_and(|item| sle.item != item) {
            continue;
        }
        if filters
            .location
            .is_some_and(|location| sle.location != location)
        {
            continue;
        }
        if filters.batch.is_some_and(|batch| sle.batch != batch) {
            continue;
        }

        let sbe = map
            .entry((sle.item.clone(), sle.location.clone(), sle.batch.clone()))
            .or_insert_with(|| StockBalanceEntry {
                item: sle.item.clone(),
                location: sle.location.clone(),
                batch: sle.batch.clone(),
                ..StockBalanceEntry::default()
            });

        if (filters.before)(sle.date) {
            sbe.opening_quantity += sle.quantity;
            sbe.opening_value += sle.value_change;
            sbe.balance_quantity += sle.quantity;
            sbe.balance_value += sle.value_change;
            continue;
        }
        if (filters.after)(sle.date) {
            continue;
        }

        sbe.balance_quantity += sle.quantity;
        sbe.balance_value += sle.value_change;
        if sle.quantity > 0.0 {
            sbe.incoming_quantity += sle.quantity;
            sbe.incoming_value += sle.value_change;
        } else {
            sbe.outgoing_quantity -= sle.quantity;
            sbe.outgoing_value -= sle.value_change;
        }
        sbe.valuation_rate = sle.valuation_rate;
    }
    map.into_values().collect()
}

/// `getShipmentCOGSAmountFromSLEs`: what a shipment's items cost, taken out of their
/// stock first in, first out as of its date. A row whose item has no stock there costs
/// its amount.
pub fn get_shipment_cogs_amount(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let date = match doc.get("date") {
        Some(DocValue::Date(date)) => *date,
        _ => Timestamp::now(),
    };
    let items: Vec<&Doc> = doc
        .table("items")
        .iter()
        .filter(|row| text(row, "item").is_some_and(|i| !i.is_empty()))
        .collect();
    let mut item_names: Vec<FilterValue> = Vec::new();
    for row in &items {
        let name = FilterValue::from(text(row, "item").unwrap_or_default());
        if !item_names.contains(&name) {
            item_names.push(name);
        }
    }

    let filters = QueryFilter::from([
        ("item".to_owned(), Filter::list("in", item_names)),
        (
            "date".to_owned(),
            Filter::op("<=", latte_db::to_iso_string(date).as_str()),
        ),
    ]);
    let mut queues: HashMap<(String, String, String), StockQueue> = HashMap::new();
    for sle in get_raw_stock_ledger_entries(fyo, filters)? {
        let batch = sle.batch.unwrap_or_else(|| "-".to_owned());
        let q = queues.entry((sle.item, sle.location, batch)).or_default();
        if sle.quantity > 0.0 {
            q.inward(sle.rate, sle.quantity);
        } else {
            q.outward(-sle.quantity);
        }
    }

    let mut total = fyo.pesa().zero();
    for row in items {
        let key = |fieldname: &str| {
            text(row, fieldname)
                .filter(|v| !v.is_empty())
                .unwrap_or("-")
                .to_owned()
        };
        let Some(q) = queues.get_mut(&(key("item"), key("location"), key("batch"))) else {
            total = total.add(&money_or_zero(row, "amount", fyo))?;
            continue;
        };

        let st_rate = money_or_zero(row, "rate", fyo).float();
        let quantity = number(row, "quantity").unwrap_or(0.0);
        let rate = q.outward(quantity).unwrap_or(st_rate);
        total = total.add(rate * quantity)?;
    }
    Ok(total)
}
