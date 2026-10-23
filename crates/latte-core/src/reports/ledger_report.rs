// Port of reports/LedgerReport.ts @ v0.36.0: the ledger entries a report reads.

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, GetAllOptions, Order, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;

/// `LedgerEntry`. `name` is negative for the rows a report adds: -1 a total, -2 the
/// closing row, -3 a blank row.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub index: Option<String>,
    pub name: i64,
    pub account: String,
    pub date: Option<Timestamp>,
    pub debit: Option<Money>,
    pub credit: Option<Money>,
    pub balance: Option<Money>,
    pub reference_type: String,
    pub reference_name: String,
    pub party: String,
    pub reverted: bool,
    pub reverts: String,
}

impl LedgerEntry {
    /// A row the report adds, with only `name` and `account`.
    pub fn added(name: i64, account: &str) -> Self {
        Self {
            index: None,
            name,
            account: account.to_owned(),
            date: None,
            debit: None,
            credit: None,
            balance: None,
            reference_type: String::new(),
            reference_name: String::new(),
            party: String::new(),
            reverted: false,
            reverts: String::new(),
        }
    }

    /// The field a report groups by: `account`, `party` or `referenceName`.
    pub fn group_key(&self, group_by: &str) -> &str {
        match group_by {
            "account" => &self.account,
            "party" => &self.party,
            _ => &self.reference_name,
        }
    }
}

/// `_getGroupedMap`'s `GroupedMap`: entries by the group they're in, in the order the
/// groups first appear.
pub type GroupedMap = IndexMap<String, Vec<LedgerEntry>>;

/// `_setRawData`: the ledger entries for `filters`, by date and then when they were made,
/// with their debits and credits made positive.
pub fn raw_ledger_entries(
    fyo: &Fyo,
    filters: QueryFilter,
    ascending: bool,
) -> Result<Vec<LedgerEntry>, Error> {
    let fields = [
        "name",
        "account",
        "date",
        "debit",
        "credit",
        "referenceType",
        "referenceName",
        "party",
        "reverted",
        "reverts",
    ];
    let options = GetAllOptions {
        fields: Some(fields.iter().map(|f| (*f).to_owned()).collect()),
        filters,
        order_by: Some(vec!["date".to_owned(), "created".to_owned()]),
        order: Some(if ascending { Order::Asc } else { Order::Desc }),
        ..GetAllOptions::default()
    };

    let text = |row: &latte_db::DocValueMap, fieldname: &str| match row.get(fieldname) {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    };
    let money = |row: &latte_db::DocValueMap, fieldname: &str| -> Result<Money, Error> {
        match row.get(fieldname) {
            Some(DocValue::Money(money)) => Ok(money.abs()?),
            _ => Ok(fyo.pesa().zero()),
        }
    };

    let mut entries = Vec::new();
    for row in fyo.db.get_all_docs("AccountingLedgerEntry", &options)? {
        entries.push(LedgerEntry {
            index: None,
            name: text(&row, "name").parse().unwrap_or(0),
            account: text(&row, "account"),
            date: match row.get("date") {
                Some(DocValue::Date(date)) => Some(*date),
                _ => None,
            },
            debit: Some(money(&row, "debit")?),
            credit: Some(money(&row, "credit")?),
            balance: Some(fyo.pesa().zero()),
            reference_type: text(&row, "referenceType"),
            reference_name: text(&row, "referenceName"),
            party: text(&row, "party"),
            reverted: matches!(row.get("reverted"), Some(DocValue::Bool(true))),
            reverts: text(&row, "reverts"),
        });
    }
    Ok(entries)
}

/// `_getGroupedMap` without sorting: the entries grouped by `group_by`.
pub fn grouped_map(entries: &[LedgerEntry], group_by: &str) -> GroupedMap {
    let mut map = GroupedMap::new();
    for entry in entries {
        map.entry(entry.group_key(group_by).to_owned())
            .or_default()
            .push(entry.clone());
    }
    map
}
