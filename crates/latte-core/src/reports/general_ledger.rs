// Port of reports/GeneralLedger/GeneralLedger.ts @ v0.36.0.
//
// Books bugs Latte fixes:
// - The To Date is included. Books compares the stored date and time with the To Date
//   alone, so the entries of the To Date itself are left out.
// - Shipments and purchase receipts are offered as reference types when inventory is on.
//   Books offers them only when it is off.

use jiff::civil::Date;
use jiff::{ToSpan, Zoned};
use latte_db::{Filter as DbFilter, FilterValue as DbFilterValue, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::models::helpers::single_is_truthy;
use crate::reports::ledger_report::{LedgerEntry, grouped_map, raw_ledger_entries};
use crate::reports::{
    Align, Column, Filter, FilterKind, FilterValue, RawValue, Report, ReportCell, ReportRow,
};

#[derive(Debug, Clone)]
pub struct GeneralLedger {
    pub ascending: bool,
    pub reverted: bool,
    /// `All`, or a schema name.
    pub reference_type: String,
    pub reference_name: Option<String>,
    pub account: Option<String>,
    pub party: Option<String>,
    pub from_date: Option<Date>,
    pub to_date: Option<Date>,
    /// `none`, `party`, `account` or `referenceName`.
    pub group_by: String,
}

impl Default for GeneralLedger {
    fn default() -> Self {
        Self {
            ascending: false,
            reverted: false,
            reference_type: "All".to_owned(),
            reference_name: None,
            account: None,
            party: None,
            from_date: None,
            to_date: None,
            group_by: "none".to_owned(),
        }
    }
}

impl GeneralLedger {
    /// `setDefaultFilters`: from a year ago to tomorrow.
    pub fn set_default_filters(&mut self, today: Date) {
        if self.to_date.is_none() {
            self.to_date = today.checked_add(1.day()).ok();
            self.from_date = today.checked_sub(1.year()).ok();
        }
    }

    /// `_getGroupByKey`.
    fn group_by_key(&self) -> &str {
        match self.group_by.as_str() {
            "none" | "" => "referenceName",
            group_by => group_by,
        }
    }

    /// `_getQueryFilters`.
    fn query_filters(&self) -> QueryFilter {
        let mut filters = QueryFilter::new();
        for (fieldname, value) in [
            ("account", &self.account),
            ("party", &self.party),
            ("referenceName", &self.reference_name),
        ] {
            if let Some(value) = value {
                filters.insert(fieldname.to_owned(), DbFilter::from(value.as_str()));
            }
        }
        if self.reference_type != "All" {
            filters.insert(
                "referenceType".to_owned(),
                DbFilter::from(self.reference_type.as_str()),
            );
        }

        let to_date = self
            .to_date
            .and_then(|date| date.checked_add(1.day()).ok())
            .map(|date| DbFilterValue::Text(date.to_string()));
        let from_date = self
            .from_date
            .map(|date| DbFilterValue::Text(date.to_string()));
        let date = match (to_date, from_date) {
            (Some(to), Some(from)) => Some(DbFilter::Two(
                "<".to_owned(),
                latte_db::FilterOperand::One(to),
                ">=".to_owned(),
                latte_db::FilterOperand::One(from),
            )),
            (Some(to), None) => Some(DbFilter::op("<", to)),
            (None, Some(from)) => Some(DbFilter::op(">=", from)),
            (None, None) => None,
        };
        if let Some(date) = date {
            filters.insert("date".to_owned(), date);
        }

        if !self.reverted {
            filters.insert(
                "reverted".to_owned(),
                DbFilter::Equals(DbFilterValue::Bool(false)),
            );
        }
        filters
    }

    /// The report's entries: `setReportData` before the rows are made.
    pub fn entries(&mut self, fyo: &Fyo) -> Result<Vec<LedgerEntry>, Error> {
        let raw = raw_ledger_entries(fyo, self.query_filters(), self.ascending)?;
        let mut map = grouped_map(&raw, self.group_by_key());
        let grouped = self.group_by != "none";

        let mut index = 1;
        for entries in map.values_mut() {
            for entry in entries.iter_mut() {
                entry.index = Some(index.to_string());
                index += 1;
            }
        }

        let zero = fyo.pesa().zero();
        let mut total_debit = zero.clone();
        let mut total_credit = zero.clone();
        for entries in map.values_mut() {
            let mut balance = zero.clone();
            let mut debit = zero.clone();
            let mut credit = zero.clone();
            for entry in entries.iter_mut() {
                let entry_debit = entry.debit.clone().unwrap_or_else(|| zero.clone());
                let entry_credit = entry.credit.clone().unwrap_or_else(|| zero.clone());
                debit = debit.add(&entry_debit)?;
                credit = credit.add(&entry_credit)?;
                balance = balance.add(&entry_debit.sub(&entry_credit)?)?;
                entry.balance = Some(balance.clone());
            }

            if grouped {
                let mut total = LedgerEntry::added(-1, "Total");
                total.balance = Some(debit.sub(&credit)?);
                total.debit = Some(debit.clone());
                total.credit = Some(credit.clone());
                entries.push(total);
            }
            total_debit = total_debit.add(&debit)?;
            total_credit = total_credit.add(&credit)?;
        }

        let mut consolidated = Vec::new();
        for entries in map.into_values() {
            consolidated.extend(entries);
            if grouped {
                consolidated.push(LedgerEntry::added(-3, ""));
            }
        }
        if consolidated.last().map(|e| e.name) != Some(-3) {
            consolidated.push(LedgerEntry::added(-3, ""));
        }

        let mut closing = LedgerEntry::added(-2, "Closing");
        closing.balance = Some(total_debit.sub(&total_credit)?);
        closing.debit = Some(total_debit);
        closing.credit = Some(total_credit);
        consolidated.push(closing);
        Ok(consolidated)
    }

    /// `_getRowFromEntry`.
    fn row(
        &self,
        fyo: &Fyo,
        formatter: &Formatter,
        entry: &LedgerEntry,
        columns: &[Column],
    ) -> ReportRow {
        if entry.name == -3 {
            return ReportRow {
                is_empty: true,
                cells: columns
                    .iter()
                    .map(|c| ReportCell {
                        width: c.width,
                        ..ReportCell::default()
                    })
                    .collect(),
                ..ReportRow::default()
            };
        }

        let cells = columns
            .iter()
            .map(|column| {
                let (value, raw_value) = match column.fieldname.as_str() {
                    "index" => match &entry.index {
                        Some(index) => (index.clone(), RawValue::Text(index.clone())),
                        None => (String::new(), RawValue::Empty),
                    },
                    "date" => match entry.date {
                        Some(date) => (formatter.timestamp_date(date), RawValue::Date(date)),
                        None => (String::new(), RawValue::Empty),
                    },
                    "debit" | "credit" | "balance" => {
                        let money = match column.fieldname.as_str() {
                            "debit" => &entry.debit,
                            "credit" => &entry.credit,
                            _ => &entry.balance,
                        };
                        match money {
                            Some(money) => {
                                (formatter.currency(money), RawValue::Money(money.clone()))
                            }
                            None => (String::new(), RawValue::Empty),
                        }
                    }
                    "reverted" => (
                        if entry.reverted { "Reverted" } else { "" }.to_owned(),
                        RawValue::Bool(entry.reverted),
                    ),
                    "referenceType" => {
                        let label = fyo
                            .schema(&entry.reference_type)
                            .map(|schema| schema.label.clone())
                            .unwrap_or_else(|| entry.reference_type.clone());
                        (label, RawValue::Text(entry.reference_type.clone()))
                    }
                    fieldname => {
                        let text = match fieldname {
                            "account" => &entry.account,
                            "party" => &entry.party,
                            _ => &entry.reference_name,
                        };
                        (text.clone(), RawValue::Text(text.clone()))
                    }
                };
                ReportCell {
                    value,
                    raw_value,
                    italics: entry.name == -1,
                    bold: entry.name == -2,
                    align: column.align,
                    width: column.width,
                    ..ReportCell::default()
                }
            })
            .collect();
        ReportRow {
            cells,
            ..ReportRow::default()
        }
    }
}

impl Report for GeneralLedger {
    fn title(&self) -> &'static str {
        "General Ledger"
    }

    fn report_name(&self) -> &'static str {
        "general-ledger"
    }

    fn filters(&self, fyo: &Fyo) -> Vec<Filter> {
        let mut ref_types = vec![
            ("All", "All"),
            ("Sales Invoices", "SalesInvoice"),
            ("Purchase Invoices", "PurchaseInvoice"),
            ("Payments", "Payment"),
            ("Journal Entries", "JournalEntry"),
        ];
        if single_is_truthy(fyo, "AccountingSettings", "enableInventory") {
            ref_types.push(("Shipment", "Shipment"));
            ref_types.push(("Purchase Receipt", "PurchaseReceipt"));
        }
        let options = |options: &[(&str, &str)]| {
            options
                .iter()
                .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
                .collect()
        };

        vec![
            Filter::new(
                "referenceType",
                "Ref Type",
                FilterKind::Select(options(&ref_types)),
            ),
            Filter::new(
                "referenceName",
                "Ref. Name",
                FilterKind::DynamicLink {
                    references: "referenceType".to_owned(),
                },
            ),
            Filter::new("account", "Account", FilterKind::Link("Account".to_owned())),
            Filter::new("party", "Party", FilterKind::Link("Party".to_owned())),
            Filter::new("fromDate", "From Date", FilterKind::Date),
            Filter::new("toDate", "To Date", FilterKind::Date),
            Filter::new(
                "groupBy",
                "Group By",
                FilterKind::Select(options(&[
                    ("None", "none"),
                    ("Party", "party"),
                    ("Account", "account"),
                    ("Reference", "referenceName"),
                ])),
            ),
            Filter::new("reverted", "Include Cancelled", FilterKind::Check),
            Filter::new("ascending", "Ascending Order", FilterKind::Check),
        ]
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "referenceType" => FilterValue::Text(self.reference_type.clone()),
            "referenceName" => FilterValue::text(self.reference_name.as_deref()),
            "account" => FilterValue::text(self.account.as_deref()),
            "party" => FilterValue::text(self.party.as_deref()),
            "fromDate" => FilterValue::date(self.from_date),
            "toDate" => FilterValue::date(self.to_date),
            "groupBy" => FilterValue::Text(self.group_by.clone()),
            "reverted" => FilterValue::Bool(self.reverted),
            "ascending" => FilterValue::Bool(self.ascending),
            _ => FilterValue::Null,
        }
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        let text = |value: &FilterValue| match value {
            FilterValue::Text(text) if !text.is_empty() => Some(text.clone()),
            _ => None,
        };
        let date = |value: &FilterValue| match value {
            FilterValue::Date(date) => Some(*date),
            _ => None,
        };
        let check = |value: &FilterValue| matches!(value, FilterValue::Bool(true));
        match fieldname {
            "referenceType" => {
                self.reference_type = text(&value).unwrap_or_else(|| "All".to_owned());
            }
            "referenceName" => self.reference_name = text(&value),
            "account" => self.account = text(&value),
            "party" => self.party = text(&value),
            "fromDate" => self.from_date = date(&value),
            "toDate" => self.to_date = date(&value),
            "groupBy" => self.group_by = text(&value).unwrap_or_else(|| "none".to_owned()),
            "reverted" => self.reverted = check(&value),
            "ascending" => self.ascending = check(&value),
            _ => {}
        }
    }

    fn columns(&self, _fyo: &Fyo, _formatter: &Formatter) -> Result<Vec<Column>, Error> {
        let mut columns = vec![
            Column::new("#", "index", Align::Right, 0.5),
            Column::new("Account", "account", Align::Left, 1.5),
            Column::new("Date", "date", Align::Left, 1.0),
            Column::new("Debit", "debit", Align::Right, 1.25),
            Column::new("Credit", "credit", Align::Right, 1.25),
            Column::new("Balance", "balance", Align::Right, 1.25),
            Column::new("Party", "party", Align::Left, 1.0),
            Column::new("Ref Name", "referenceName", Align::Left, 1.0),
            Column::new("Ref Type", "referenceType", Align::Left, 1.0),
        ];
        if self.reverted {
            columns.push(Column::new("Reverted", "reverted", Align::Left, 1.0));
        }
        Ok(columns)
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.set_default_filters(Zoned::now().date());
        let columns = self.columns(fyo, formatter)?;
        let entries = self.entries(fyo)?;
        Ok(entries
            .iter()
            .map(|entry| self.row(fyo, formatter, entry, &columns))
            .collect())
    }
}
